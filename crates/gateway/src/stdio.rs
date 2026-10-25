use std::time::Duration;

use futures_util::StreamExt;
use reqwest::{Client, StatusCode, redirect::Policy};
use serde_json::{Value, json};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, stdin, stdout};
use tokio_util::codec::{FramedRead, LinesCodec};
use url::Url;

use crate::stdio_auth::{StdioAuthError, TokenProvider, normalize_server_url};

const MAX_LINE_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

pub(crate) struct RemoteMcpClient {
    endpoint: Url,
    client: Client,
    session_id: Option<String>,
    version: Option<String>,
}

impl RemoteMcpClient {
    pub(crate) fn new(server: &str) -> Result<Self, StdioAuthError> {
        let endpoint = normalize_server_url(server)?
            .join("mcp")
            .map_err(|_| StdioAuthError::InvalidServer)?;
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(315))
            .build()
            .map_err(|_| StdioAuthError::RemoteTransport)?;
        Ok(Self {
            endpoint,
            client,
            session_id: None,
            version: None,
        })
    }

    pub(crate) async fn send(
        &mut self,
        token: &str,
        message: &Value,
    ) -> Result<Vec<Value>, StdioAuthError> {
        let initializing = message.get("method").and_then(Value::as_str) == Some("initialize");
        let mut request = self
            .client
            .post(self.endpoint.clone())
            .bearer_auth(token)
            .header("accept", "application/json, text/event-stream")
            .json(message);
        if !initializing {
            if let Some(session_id) = &self.session_id {
                request = request.header("mcp-session-id", session_id);
            }
            if let Some(version) = &self.version {
                request = request.header("mcp-protocol-version", version);
            }
        }
        let response = request
            .send()
            .await
            .map_err(|_| StdioAuthError::RemoteTransport)?;
        if response.status() == StatusCode::UNAUTHORIZED {
            return Err(StdioAuthError::ReauthenticationRequired);
        }
        if response.status() == StatusCode::NOT_FOUND && self.session_id.is_some() {
            self.session_id = None;
            self.version = None;
            return Err(StdioAuthError::SessionExpired);
        }
        if !response.status().is_success() {
            return Err(StdioAuthError::RemoteTransport);
        }
        if response.status() == StatusCode::ACCEPTED {
            if message.get("method").is_some()
                && !message
                    .as_object()
                    .is_some_and(|object| object.contains_key("id"))
            {
                return Ok(Vec::new());
            }
            // Client responses also receive 202, never ordinary requests.
            if message.get("method").is_none() {
                return Ok(Vec::new());
            }
            return Err(StdioAuthError::RemoteTransport);
        }
        let session_id = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .unwrap_or_default()
            .to_owned();
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| StdioAuthError::RemoteTransport)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(StdioAuthError::RemoteTransport);
            }
            bytes.extend_from_slice(&chunk);
        }
        let messages = match content_type.as_str() {
            "application/json" => vec![
                serde_json::from_slice::<Value>(&bytes)
                    .map_err(|_| StdioAuthError::RemoteTransport)?,
            ],
            "text/event-stream" => parse_sse(&bytes)?,
            _ => return Err(StdioAuthError::RemoteTransport),
        };
        let response = messages
            .iter()
            .find(|response| {
                response.get("id") == message.get("id")
                    && (response.get("result").is_some() || response.get("error").is_some())
            })
            .ok_or(StdioAuthError::RemoteTransport)?;
        if initializing
            && let Some(version) = response
                .pointer("/result/protocolVersion")
                .and_then(Value::as_str)
        {
            self.version = Some(version.to_owned());
            self.session_id = session_id;
        }
        Ok(messages)
    }

    async fn close(&self, token: &str) {
        if let Some(session_id) = &self.session_id {
            let response = self
                .client
                .delete(self.endpoint.clone())
                .bearer_auth(token)
                .header("mcp-session-id", session_id)
                .header(
                    "mcp-protocol-version",
                    self.version.as_deref().unwrap_or("2025-11-25"),
                )
                .timeout(Duration::from_secs(5))
                .send()
                .await;
            if !response.is_ok_and(|response| response.status().is_success()) {
                tracing::warn!(
                    "could not terminate remote MCP session; it will expire automatically"
                );
            }
        }
    }
}

fn parse_sse(bytes: &[u8]) -> Result<Vec<Value>, StdioAuthError> {
    let text = std::str::from_utf8(bytes).map_err(|_| StdioAuthError::RemoteTransport)?;
    let mut messages = Vec::new();
    let mut data = String::new();
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !data.is_empty() {
                messages.push(
                    serde_json::from_str::<Value>(&data)
                        .map_err(|_| StdioAuthError::RemoteTransport)?,
                );
                data.clear();
            }
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value.strip_prefix(' ').unwrap_or(value));
        }
    }
    Ok(messages)
}

pub async fn run(server: &str) -> Result<(), StdioAuthError> {
    let mut remote = RemoteMcpClient::new(server)?;
    let tokens = TokenProvider::load(server).await?;
    bridge(stdin(), stdout(), &mut remote, &tokens).await;
    if let Ok(token) = tokens.access_token().await {
        remote.close(&token).await;
    }
    Ok(())
}

async fn bridge<R: AsyncRead + Unpin, W: AsyncWrite + Unpin>(
    input: R,
    mut output: W,
    remote: &mut RemoteMcpClient,
    tokens: &TokenProvider,
) {
    let mut lines = FramedRead::new(input, LinesCodec::new_with_max_length(MAX_LINE_BYTES));
    while let Some(line) = lines.next().await {
        let request = match line
            .ok()
            .and_then(|line| serde_json::from_str::<Value>(&line).ok())
        {
            Some(Value::Object(object)) => Value::Object(object),
            _ => {
                if write_message(&mut output, &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32600,"message":"Invalid Request"}})).await.is_err() {
                    break;
                }
                continue;
            }
        };
        let result = match tokens.access_token().await {
            Ok(token) => remote.send(&token, &request).await,
            Err(error) => Err(error),
        };
        let messages = match result {
            Ok(messages) => messages,
            Err(error) => {
                tracing::error!(error = %error, "remote MCP request failed; request was not retried");
                match transport_error(&request, &error) {
                    Some(message) => vec![message],
                    None => Vec::new(),
                }
            }
        };
        for message in messages {
            if write_message(&mut output, &message).await.is_err() {
                return;
            }
        }
    }
}

async fn write_message<W: AsyncWrite + Unpin>(
    output: &mut W,
    value: &Value,
) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(value)?;
    bytes.push(b'\n');
    output.write_all(&bytes).await?;
    output.flush().await
}

fn transport_error(request: &Value, error: &StdioAuthError) -> Option<Value> {
    request.get("method")?;
    let id = request.get("id")?;
    Some(json!({"jsonrpc":"2.0", "id":id, "error":{"code":-32001, "message":error.to_string()}}))
}

#[cfg(test)]
mod tests {
    use axum::{
        Json, Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post,
    };
    use std::sync::Arc;
    use tokio::{net::TcpListener, sync::Mutex};

    use super::*;

    type CapturedMessages = Arc<Mutex<Vec<(HeaderMap, Value)>>>;

    async fn fake_gateway(
        State(captured): State<CapturedMessages>,
        headers: HeaderMap,
        Json(message): Json<Value>,
    ) -> axum::response::Response {
        captured.lock().await.push((headers, message.clone()));
        match message["method"].as_str() {
            Some("initialize") => (
                [("mcp-session-id", "synthetic-session")],
                Json(json!({"jsonrpc":"2.0", "id":message["id"], "result":{"protocolVersion":"2025-11-25"}})),
            ).into_response(),
            Some("notifications/initialized") => axum::http::StatusCode::ACCEPTED.into_response(),
            Some("tools/list") => (
                [("content-type", "text/event-stream")],
                format!("event: message\ndata: {}\n\n", json!({"jsonrpc":"2.0","id":message["id"],"result":{"tools":[]}})),
            ).into_response(),
            _ => axum::http::StatusCode::NOT_FOUND.into_response(),
        }
    }

    #[tokio::test]
    async fn remote_client_preserves_session_version_and_sse_without_retrying_failed_calls() {
        let captured: CapturedMessages = Arc::default();
        let app = Router::new()
            .route("/mcp", post(fake_gateway))
            .with_state(captured.clone());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut remote = RemoteMcpClient::new(&url).unwrap();
        let initialize = json!({"jsonrpc":"2.0","id":1,"method":"initialize"});
        remote
            .send("synthetic-access-token", &initialize)
            .await
            .unwrap();
        let result = remote
            .send(
                "synthetic-access-token",
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            )
            .await
            .unwrap();
        assert!(result.is_empty());
        let result = remote
            .send(
                "synthetic-access-token",
                &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
            )
            .await
            .unwrap();
        assert_eq!(result[0]["result"]["tools"], json!([]));
        assert!(matches!(
            remote
                .send(
                    "synthetic-access-token",
                    &json!({"jsonrpc":"2.0","id":3,"method":"tools/call"})
                )
                .await,
            Err(StdioAuthError::SessionExpired)
        ));
        assert!(remote.session_id.is_none());
        let captured = captured.lock().await;
        assert_eq!(captured.len(), 4, "failed calls must never be retried");
        assert!(!captured[0].0.contains_key("mcp-session-id"));
        for (headers, _) in captured.iter().skip(1) {
            assert_eq!(headers["mcp-session-id"], "synthetic-session");
            assert_eq!(headers["mcp-protocol-version"], "2025-11-25");
            assert_eq!(headers["accept"], "application/json, text/event-stream");
        }
        server.abort();
    }

    #[test]
    fn finite_sse_accepts_comments_crlf_and_multiline_data() {
        let values = parse_sse(b": keep-alive\r\nevent: message\r\ndata: {\r\ndata: \"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\r\n\r\n").unwrap();
        assert_eq!(values, vec![json!({"jsonrpc":"2.0", "id":1,"result":{}})]);
        assert!(parse_sse(b"data: invalid\n\n").is_err());
    }

    #[test]
    fn transport_errors_preserve_ids_without_echoing_request_data() {
        let request = json!({"jsonrpc":"2.0", "id":7,"method":"tools/call","access_token":"synthetic-secret"});
        let response = transport_error(&request, &StdioAuthError::RemoteTransport).unwrap();
        assert_eq!(response["id"], 7);
        assert!(!response.to_string().contains("synthetic-secret"));
        assert!(
            transport_error(
                &json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                &StdioAuthError::RemoteTransport
            )
            .is_none()
        );
    }
}
