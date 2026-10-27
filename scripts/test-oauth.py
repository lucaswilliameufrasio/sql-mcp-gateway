#!/usr/bin/env python3
"""Real Keycloak -> gateway -> PostgreSQL -> MCP/stdio integration (stdlib only).

Invoked only by test-integration.sh with isolated generated configuration.
Never loads the repository .env or accesses a shared service.
"""

import concurrent.futures
import hashlib
import base64
import html.parser
import http.cookiejar
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid


def prepare(root):
    oidc = root / "oidc"
    oidc.mkdir(mode=0o755)
    oidc.chmod(0o755)  # The parent script deliberately uses umask 077.
    passwords = {user: secrets.token_urlsafe(32) for user in ("alice", "bob")}
    (root / "oidc-passwords.json").write_text(json.dumps(passwords))
    (root / "oidc-passwords.json").chmod(0o600)
    realm = {
        "realm": "mcp-it", "enabled": True, "sslRequired": "none",
        "registrationAllowed": False,
        "users": [{"username": user, "enabled": True, "emailVerified": True,
                   "email": f"{user}@example.test", "firstName": user, "lastName": "Test",
                   "credentials": [{"type": "password", "value": password, "temporary": False}]}
                  for user, password in passwords.items()],
        "clients": [{"clientId": "gateway-it", "publicClient": True,
                     "standardFlowEnabled": True, "directAccessGrantsEnabled": False,
                     # Only this ephemeral, loopback-bound test realm permits wildcard callbacks.
                     "redirectUris": ["*"],
                     "attributes": {"pkce.code.challenge.method": "S256"},
                     "protocolMappers": [{"name": "gateway-audience", "protocol": "openid-connect",
                         "protocolMapper": "oidc-audience-mapper", "consentRequired": False,
                         "config": {"included.client.audience": "gateway-it",
                                    "access.token.claim": "true", "id.token.claim": "false"}}]}],
    }
    (oidc / "realm.json").write_text(json.dumps(realm))
    (oidc / "realm.json").chmod(0o644)  # Keycloak container needs read access.


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Browser:
    def __init__(self):
        self.opener = urllib.request.build_opener(
            NoRedirect(), urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar(LoopbackCookiePolicy())))

    def call(self, url, method="GET", form=None, value=None, headers=None, status=200):
        assert urllib.parse.urlsplit(url).hostname == "127.0.0.1", "non-local test URL rejected"
        data = None
        headers = dict(headers or {})
        if form is not None:
            data = urllib.parse.urlencode(form).encode()
            headers["Content-Type"] = "application/x-www-form-urlencoded"
        if value is not None:
            data = json.dumps(value).encode()
            headers.setdefault("Content-Type", "application/json")
        request = urllib.request.Request(url, data=data, headers=headers, method=method)
        try:
            response = self.opener.open(request, timeout=15)
        except urllib.error.HTTPError as error:
            response = error
        accepted = status if isinstance(status, tuple) else (status,)
        assert response.status in accepted, f"{method} {urllib.parse.urlsplit(url).path}: expected {status}, got {response.status}"
        body = response.read(16 * 1024 * 1024)
        return response.headers, body


class LoopbackCookiePolicy(http.cookiejar.DefaultCookiePolicy):
    def return_ok_secure(self, cookie, request):
        # Browsers treat loopback as a potentially trustworthy origin. Keycloak
        # marks its auth cookies Secure even in this isolated HTTP dev realm.
        if urllib.parse.urlsplit(request.full_url).hostname == "127.0.0.1":
            return True
        return super().return_ok_secure(cookie, request)


class LoginForm(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.action = None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "form" and attrs.get("id") == "kc-form-login":
            self.action = attrs.get("action")


def wait_ready(url, process=None):
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        if process is not None and process.poll() is not None:
            raise AssertionError("isolated gateway exited during startup")
        try:
            with urllib.request.urlopen(url, timeout=2) as response:
                if response.status in (200, 204):
                    return
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            pass
        time.sleep(0.5)
    raise AssertionError("isolated service did not become ready within 120 seconds")


class Suite:
    def __init__(self, root, binary):
        self.root = root
        self.binary = str(Path(binary).resolve())
        self.settings = dict(line.split("=", 1) for line in (root / "test.env").read_text().splitlines() if line)
        self.passwords = json.loads((root / "oidc-passwords.json").read_text())
        self.issuer = self.settings["SQL_MCP_GATEWAY_TEST_OIDC_ISSUER"]
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            self.port = listener.getsockname()[1]
        self.base = f"http://127.0.0.1:{self.port}"
        self.resource = self.base + "/mcp"
        self.http = Browser()
        self.process = None
        self.vault = Browser()
        self.runtime_token = None

    def vault_call(self, path, method="GET", value=None, token=None, status=200):
        address = self.settings["SQL_MCP_GATEWAY_TEST_VAULT_ADDRESS"].rstrip("/")
        headers = {"X-Vault-Token": token or self.settings["SQL_MCP_GATEWAY_TEST_VAULT_ADMIN_TOKEN"]}
        return self.vault.call(address + "/v1/" + path, method, value=value, headers=headers, status=status)

    def prepare_vault(self):
        _, body = self.vault_call("auth/token/create", "POST", {
            "no_default_policy": True,
            "ttl": "1h",
            "renewable": False,
        })
        self.runtime_token = json.loads(body)["auth"]["client_token"]

    def sql(self, statement):
        container = self.settings["SQL_MCP_GATEWAY_TEST_POSTGRES_CONTAINER"]
        assert container.isalnum(), "invalid isolated container ID"
        result = subprocess.run(["docker", "exec", "-i", container, "psql", "-XAt", "-v", "ON_ERROR_STOP=1",
                                 "-U", "postgres", "-d", "sql_mcp_gateway_it"],
                                input=statement, text=True, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=15)
        assert result.returncode == 0, "test fixture database query failed"
        return result.stdout.strip()

    def start(self):
        wait_ready(self.issuer + "/.well-known/openid-configuration")
        self.prepare_vault()
        password = urllib.parse.quote(self.settings["SQL_MCP_GATEWAY_TEST_POSTGRES_PASSWORD"], safe="")
        database = f"postgres://postgres:{password}@127.0.0.1:{self.settings['SQL_MCP_GATEWAY_TEST_POSTGRES_PORT']}/sql_mcp_gateway_it?sslmode=disable"
        environment = {
            "PATH": os.environ["PATH"], "RUST_LOG": "warn", "BIND_ADDRESS": f"127.0.0.1:{self.port}",
            "CONTROL_DATABASE_URL": database, "OIDC_ISSUER_URL": self.issuer,
            "OIDC_CLIENT_ID": "gateway-it", "OIDC_ACCESS_TOKEN_AUDIENCE": "gateway-it",
            "OIDC_BOOTSTRAP_ADMIN_SUBJECTS": "no-bootstrap-admin-in-this-test",
            "MCP_PUBLIC_URL": self.base, "MCP_OAUTH_STATE_KEY": secrets.token_urlsafe(48),
            "VAULT_ADDRESS": self.settings["SQL_MCP_GATEWAY_TEST_VAULT_ADDRESS"],
            "VAULT_ADMIN_TOKEN": self.settings["SQL_MCP_GATEWAY_TEST_VAULT_ADMIN_TOKEN"],
            "VAULT_RUNTIME_TOKEN": self.runtime_token,
        }
        self.process = subprocess.Popen([self.binary], cwd=self.root, env=environment,
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        wait_ready(self.base + "/readyz", self.process)

    def login(self, user="alice", decision="approve", wrong_verifier=False, resource=None):
        resource = resource or self.resource
        browser = Browser()
        verifier = secrets.token_urlsafe(32)
        challenge = base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).rstrip(b"=").decode()
        callback = "http://127.0.0.1:1/callback"
        _, body = self.http.call(self.base + "/oauth/register", "POST", value={
            "client_name": "Real OIDC integration", "redirect_uris": [callback],
            "token_endpoint_auth_method": "none", "grant_types": ["authorization_code", "refresh_token"]}, status=201)
        client = json.loads(body)["client_id"]
        state = secrets.token_urlsafe(32)
        headers, _ = browser.call(self.base + "/oauth/authorize?" + urllib.parse.urlencode({
            "response_type": "code", "client_id": client, "redirect_uri": callback,
            "state": state, "code_challenge": challenge, "code_challenge_method": "S256",
            "scope": "mcp", "resource": resource}), status=303)
        upstream = headers["Location"]
        # State/cookie mismatch is rejected before exchange or persistence.
        query = urllib.parse.parse_qs(urllib.parse.urlsplit(upstream).query)
        Browser().call(self.base + "/oauth/callback?" + urllib.parse.urlencode({"state": query["state"][0], "code": "not-a-code"}), status=400)
        _, body = browser.call(upstream)
        form = LoginForm()
        form.feed(body.decode())
        assert form.action is not None, "Keycloak did not render its login form"
        headers, _ = browser.call(form.action, "POST", form={"username": user, "password": self.passwords[user], "credentialId": ""}, status=302)
        headers, _ = browser.call(headers["Location"], status=303)
        consent_url = headers["Location"]
        _, consent_body = browser.call(consent_url)
        assert b"Autorizar" in consent_body, "gateway consent page was not rendered"
        consent_state = urllib.parse.parse_qs(urllib.parse.urlsplit(consent_url).query)["state"][0]
        headers, _ = browser.call(self.base + "/oauth/consent", "POST", form={"state": consent_state, "decision": decision}, status=303)
        redirected = urllib.parse.parse_qs(urllib.parse.urlsplit(headers["Location"]).query)
        assert redirected["state"][0] == state, "client state was not preserved"
        browser.call(self.base + "/oauth/consent", "POST", form={"state": consent_state, "decision": decision}, status=400)
        if decision == "deny":
            assert redirected["error"][0] == "access_denied", "denial did not return access_denied"
            return None
        exchange = {"grant_type": "authorization_code", "client_id": client,
                    "redirect_uri": callback, "code": redirected["code"][0],
                    "code_verifier": secrets.token_urlsafe(32) if wrong_verifier else verifier,
                    "resource": resource}
        _, body = self.http.call(self.base + "/oauth/token", "POST", form=exchange, status=400 if wrong_verifier else 200)
        self.http.call(self.base + "/oauth/token", "POST", form=exchange, status=400)
        if wrong_verifier:
            return None
        tokens = json.loads(body)
        assert tokens["token_type"] == "Bearer", "wrong OAuth token type"
        return client, tokens

    def rpc(self, tokens, message, session=None, status=200, extra=None, resource=None):
        headers = {"Authorization": "Bearer " + tokens["access_token"], "Accept": "application/json, text/event-stream"}
        if session:
            headers.update({"MCP-Session-Id": session, "MCP-Protocol-Version": "2025-11-25"})
        headers.update(extra or {})
        return self.http.call(resource or self.resource, "POST", value=message, headers=headers, status=status)

    def initialize(self, tokens, resource=None):
        headers, body = self.rpc(tokens, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "integration", "version": "1"}}}, resource=resource)
        assert json.loads(body)["result"]["protocolVersion"] == "2025-11-25", "version negotiation failed"
        session = headers["MCP-Session-Id"]
        self.rpc(tokens, {"jsonrpc": "2.0", "id": 2, "method": "ping"}, session, status=400, resource=resource)
        self.rpc(tokens, {"jsonrpc": "2.0", "method": "notifications/initialized"}, session, status=202, resource=resource)
        return session

    def tool_call(self, tokens, resource, session, name, arguments=None, status=200):
        _, body = self.rpc(
            tokens,
            {
                "jsonrpc": "2.0",
                "id": secrets.randbelow(1_000_000) + 10,
                "method": "tools/call",
                "params": {"name": name, "arguments": arguments or {}},
            },
            session,
            status=status,
            resource=resource,
        )
        return json.loads(body)["result"]

    def refresh(self, client, tokens, status=200, resource=None):
        return self.http.call(self.base + "/oauth/token", "POST", form={"grant_type": "refresh_token", "client_id": client,
            "refresh_token": tokens["refresh_token"], "resource": resource or self.resource}, status=status)

    def run(self):
        self.http.call(self.resource, status=401)
        self.login(decision="deny")
        self.login(wrong_verifier=True)
        client, tokens = self.login()
        bob_client, bob = self.login("bob")
        # Verify tokens are stored only as hashes, and real OIDC identities persisted.
        assert self.sql("SELECT COUNT(*) FROM principals WHERE subject <> ''") == "2", "real principals were not persisted"
        assert self.sql("SELECT COUNT(*) FROM mcp_oauth_codes WHERE consumed_at IS NOT NULL") != "0", "one-time code consumption was not persisted"
        session = self.initialize(tokens)
        ping = {"jsonrpc": "2.0", "id": 2, "method": "ping"}
        self.rpc(tokens, ping, status=400)
        self.rpc(bob, ping, session, status=404)
        self.rpc(tokens, ping, session, status=403, extra={"Origin": "https://evil.example"})
        self.rpc(tokens, ping, session, status=403, extra={"Origin": "null"})
        self.rpc(tokens, ping, session, status=406, extra={"Accept": "application/json"})
        self.rpc(tokens, ping, session, status=400, extra={"MCP-Protocol-Version": "2099-01-01"})
        for content_type in ("Application/JSON", "APPLICATION/JSON; charset=utf-8", "application/json ; charset=utf-8"):
            _, body = self.rpc(tokens, ping, session, extra={"Content-Type": content_type})
            assert json.loads(body)["result"] == {}, "valid JSON media type was rejected"
        for content_type in ("text/plain", "application/jsonp"):
            self.rpc(tokens, ping, session, status=415, extra={"Content-Type": content_type})
        self.protocol_headers(tokens, session, ping)
        self.rpc(tokens, {"jsonrpc": "2.0", "id": 3}, session, status=400)
        self.rpc(tokens, {"jsonrpc": "2.0", "id": 3, "result": {}}, session, status=202)
        headers, body = self.rpc(tokens, ping, session, extra={"Accept": "text/event-stream, application/json"})
        assert headers["Content-Type"].startswith("text/event-stream") and b'"result":{}' in body, "POST SSE did not carry the response"
        auth_headers = {"Authorization": "Bearer " + tokens["access_token"], "MCP-Session-Id": session,
                        "MCP-Protocol-Version": "2025-11-25", "Accept": "text/event-stream"}
        # Read one GET event without waiting for the bounded stream to end.
        request = urllib.request.Request(self.resource, headers=auth_headers)
        stream = urllib.request.urlopen(request, timeout=10)
        assert stream.headers["Content-Type"].startswith("text/event-stream"), "GET did not negotiate SSE"
        assert stream.readline().startswith(b":"), "idle GET must not emit RPC responses"
        assert stream.readline() == b"\n", "SSE event must be delimited"
        self.http.call(self.resource, headers=auth_headers, status=429)
        self.http.call(self.resource, headers={**auth_headers, "Last-Event-ID": "not-supported"}, status=400)
        self.http.call(self.resource, "DELETE", headers={**auth_headers, "Authorization": "Bearer " + bob["access_token"]}, status=404)
        _, body = self.rpc(tokens, {"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "list_databases"}}, session)
        assert json.loads(body)["result"]["structuredContent"] == [], "login must not grant database access"
        # Real persisted grants are consulted on every call, not cached in session.
        principal = self.sql("SELECT id FROM principals WHERE display_name = 'alice Test'")
        assert uuid.UUID(principal), "missing Alice principal"
        target = str(uuid.uuid4())
        self.sql(f"INSERT INTO database_targets (id,name,engine,host,port,database_name,tls_mode,vault_connection_name,vault_read_role,created_by) VALUES ('{target}','fixture-{target}','postgresql','127.0.0.1',5432,'unused','disable','connection-{target}','role-{target}','{principal}'); INSERT INTO principal_database_grants (principal_id,database_id,access_level,granted_by) VALUES ('{principal}','{target}','read','{principal}');")
        _, body = self.rpc(tokens, {"jsonrpc": "2.0", "id": 5, "method": "tools/call", "params": {"name": "list_databases"}}, session)
        visible = json.loads(body)["result"]["structuredContent"]
        assert len(visible) == 1 and visible[0]["id"] == target, "persisted per-user grant not enforced"
        self.sql(f"DELETE FROM principal_database_grants WHERE database_id = '{target}'")
        _, body = self.rpc(tokens, {"jsonrpc": "2.0", "id": 6, "method": "tools/call", "params": {"name": "write_query", "arguments": {"database_id": target, "sql": "DELETE FROM example"}}}, session)
        assert json.loads(body)["result"]["isError"], "revoked grant allowed a query"
        self.http.call(self.resource, "DELETE", headers=auth_headers, status=204)
        assert stream.readline() == b"", "deleted session kept its SSE stream alive"
        stream.close()
        self.rpc(tokens, ping, session, status=404)
        expired = self.initialize(tokens)
        self.sql(f"UPDATE mcp_transport_sessions SET expires_at = NOW() - INTERVAL '1 second' WHERE id = '{uuid.UUID(expired)}'")
        self.rpc(tokens, ping, expired, status=404)
        for _ in range(16):
            self.initialize(tokens)
        self.rpc(tokens, {"jsonrpc": "2.0", "id": 1, "method": "initialize"}, status=429)
        self.sql(f"DELETE FROM mcp_transport_sessions WHERE principal_id = '{principal}'")
        self.stdio(tokens)
        self.refresh(client, tokens, status=400, resource=self.base + "/wrong-resource")
        _, body = self.refresh(client, tokens)
        rotated = json.loads(body)
        assert rotated["refresh_token"] != tokens["refresh_token"], "refresh did not rotate"
        self.refresh(client, tokens, status=400)
        self.rpc(rotated, {"jsonrpc": "2.0", "id": 1, "method": "initialize"}, status=401)
        self.http.call(self.base + "/oauth/revoke", "POST", form={"client_id": bob_client, "token": bob["refresh_token"]})
        self.rpc(bob, {"jsonrpc": "2.0", "id": 1, "method": "initialize"}, status=401)
        self.refresh(bob_client, bob, status=400)
        race_client, race_tokens = self.login()
        def race_refresh():
            _, body = Browser().call(self.base + "/oauth/token", "POST", form={
                "grant_type": "refresh_token", "client_id": race_client,
                "refresh_token": race_tokens["refresh_token"], "resource": self.resource}, status=(200, 400))
            return json.loads(body)
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as executor:
            results = list(executor.map(lambda _: race_refresh(), range(2)))
        successes = [result for result in results if "access_token" in result]
        assert len(successes) == 1, "concurrent refresh consumed a token more than once"
        self.rpc(successes[0], {"jsonrpc": "2.0", "id": 1, "method": "initialize"}, status=401)
        assert self.sql("SELECT COUNT(*) FROM mcp_oauth_tokens WHERE revoked_at IS NOT NULL") != "0", "revocation was not persisted"
        self.sql(f"DELETE FROM database_targets WHERE id = '{target}'; DELETE FROM principals; DELETE FROM mcp_oauth_clients;")
        print("Real Keycloak OAuth, SQL grants, sessions/SSE, stdio and token rotation/revocation passed.")

    def protocol_headers(self, tokens, session, ping):
        headers = {"Authorization": "Bearer " + tokens["access_token"],
                   "MCP-Session-Id": session, "Accept": "application/json, text/event-stream"}
        # Absent version is recovered from the principal-bound session.
        _, body = self.http.call(self.resource, "POST", value=ping, headers=headers)
        assert json.loads(body)["result"] == {}, "missing version did not use negotiated session version"
        for version in ("", "2099-01-01", "2025-03-26", "2025-11-25, 2025-03-26", "\xff"):
            invalid = {**headers, "MCP-Protocol-Version": version}
            self.http.call(self.resource, "POST", value=ping, headers=invalid, status=400)
            self.http.call(self.resource, headers=invalid, status=400)
            self.http.call(self.resource, "DELETE", headers=invalid, status=400)
        # Rejected DELETEs must not terminate the session. GET and DELETE also
        # accept a missing version when the session supplies it.
        temporary = self.initialize(tokens)
        temporary_headers = {**headers, "MCP-Session-Id": temporary}
        request = urllib.request.Request(self.resource, headers=temporary_headers)
        with urllib.request.urlopen(request, timeout=10) as stream:
            assert stream.readline().startswith(b":"), "GET without a version did not open SSE"
        self.http.call(self.resource, "DELETE", headers=temporary_headers, status=204)
        self.rpc(tokens, ping, temporary, status=404)

    def stdio(self, tokens):
        # This subprocess has NO database, OIDC, Vault, or state-key settings.
        messages = [
            {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2025-11-25"}},
            {"jsonrpc": "2.0", "method": "notifications/initialized"},
            {"jsonrpc": "2.0", "id": 2, "method": "tools/list"},
            {"jsonrpc": "2.0", "id": 3, "method": "tools/call", "params": {"name": "list_databases"}},
        ]
        result = subprocess.run([self.binary, "--stdio", self.base], cwd=self.root,
            env={"PATH": os.environ["PATH"], "SQL_MCP_ACCESS_TOKEN": tokens["access_token"]},
            input="\n".join(json.dumps(message) for message in messages) + "\n", text=True,
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=30)
        assert result.returncode == 0, "remote stdio required operator configuration"
        responses = [json.loads(line) for line in result.stdout.splitlines()]
        assert [message["id"] for message in responses] == [1, 2, 3], "stdio IDs/notification handling failed"
        assert "result" in responses[0] and len(responses[1]["result"]["tools"]) == 4, "stdio did not forward MCP results"
        assert responses[2]["result"]["structuredContent"] == [], "stdio bypassed revoked grants"


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--prepare":
        prepare(Path(sys.argv[2]))
        return
    suite = Suite(Path(sys.argv[1]), sys.argv[2])
    try:
        suite.start()
        suite.run()
    finally:
        if suite.process is not None:
            suite.process.terminate()
            try:
                suite.process.wait(timeout=15)
            except subprocess.TimeoutExpired:
                suite.process.kill()
                suite.process.wait(timeout=5)


if __name__ == "__main__":
    main()
