use std::{convert::Infallible, time::Duration};

use axum::{
    Extension, Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, Uri},
    response::{IntoResponse, Response, Sse, sse::Event},
};
use futures_util::stream;
use serde_json::{Map, Value, json};
use sqlx::FromRow;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    auth::{self, AuthenticatedPrincipal},
    mcp, oauth,
    state::AppState,
};

const SESSION_HEADER: &str = "mcp-session-id";
const VERSION_HEADER: &str = "mcp-protocol-version";

pub(crate) fn valid_origin(headers: &HeaderMap, state: &AppState) -> bool {
    let mut origins = headers.get_all("origin").iter();
    let Some(origin) = origins.next() else {
        return true;
    };
    origins.next().is_none()
        && origin
            .to_str()
            .is_ok_and(|value| value == state.mcp_public_url.origin().ascii_serialization())
}

fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

fn json_content_type(headers: &HeaderMap) -> bool {
    header_text(headers, "content-type")
        .and_then(|value| value.split(';').next())
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

fn protocol_version(headers: &HeaderMap) -> Result<Option<&str>, StatusCode> {
    let mut values = headers.get_all(VERSION_HEADER).iter();
    let Some(value) = values.next() else {
        // A session identifies the negotiated version even if an older client
        // omits the header (MCP transport backwards compatibility).
        return Ok(None);
    };
    if values.next().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let version = value.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
    if !mcp::supported_protocol_version(version) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Some(version))
}

fn accepts(headers: &HeaderMap, wanted: &str) -> bool {
    header_text(headers, "accept").is_some_and(|accept| {
        accept.split(',').any(|item| {
            let mut parts = item.split(';');
            parts
                .next()
                .is_some_and(|media| media.trim().eq_ignore_ascii_case(wanted))
                && !parts.any(|parameter| parameter.trim() == "q=0")
        })
    })
}

#[derive(FromRow)]
struct Session {
    id: Uuid,
    protocol_version: String,
    initialized: bool,
}

async fn session(
    state: &AppState,
    actor: &AuthenticatedPrincipal,
    headers: &HeaderMap,
) -> Result<Session, StatusCode> {
    let id = header_text(headers, SESSION_HEADER)
        .ok_or(StatusCode::BAD_REQUEST)?
        .parse::<Uuid>()
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let session = sqlx::query_as::<_, Session>(
        "SELECT id, protocol_version, initialized FROM mcp_transport_sessions \
         WHERE id = $1 AND principal_id = $2 AND expires_at > NOW()",
    )
    .bind(id)
    .bind(actor.0.id)
    .fetch_optional(&state.control_pool)
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
    .ok_or(StatusCode::NOT_FOUND)?;
    if protocol_version(headers)?.is_some_and(|version| version != session.protocol_version) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(session)
}

async fn create_session(
    state: &AppState,
    actor: &AuthenticatedPrincipal,
    version: &str,
) -> Result<Uuid, StatusCode> {
    let mut transaction = state
        .control_pool
        .begin()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    // Serialize admission across replicas for this principal.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(actor.0.id.to_string())
        .execute(&mut *transaction)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    sqlx::query("DELETE FROM mcp_transport_sessions WHERE expires_at <= NOW()")
        .execute(&mut *transaction)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM mcp_transport_sessions WHERE principal_id = $1")
            .bind(actor.0.id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if count >= 16 {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO mcp_transport_sessions (id, principal_id, protocol_version) VALUES ($1, $2, $3)")
        .bind(id).bind(actor.0.id).bind(version).execute(&mut *transaction).await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    transaction
        .commit()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(id)
}

fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "error":{"code":code, "message":message}})
}

fn valid_message(message: &Map<String, Value>) -> bool {
    if message.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return false;
    }
    let valid_id = message
        .get("id")
        .is_none_or(|id| id.is_string() || id.is_number());
    if message.get("method").is_some() {
        return valid_id
            && message.get("method").is_some_and(Value::is_string)
            && !message.contains_key("result")
            && !message.contains_key("error")
            && message
                .get("params")
                .is_none_or(|params| params.is_object() || params.is_array());
    }
    valid_id
        && message.contains_key("id")
        && (message.contains_key("result") != message.contains_key("error"))
}

pub async fn post_message(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthenticatedPrincipal>,
    headers: HeaderMap,
    _uri: Uri,
    body: Bytes,
) -> Response {
    if !json_content_type(&headers) {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if !accepts(&headers, "application/json") || !accepts(&headers, "text/event-stream") {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }
    if let Err(status) = protocol_version(&headers) {
        return status.into_response();
    }
    let request = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(request)) if valid_message(&request) => request,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(rpc_error(Value::Null, -32600, "Invalid Request")),
            )
                .into_response();
        }
    };
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str);
    if method == Some("initialize") {
        if headers.contains_key(SESSION_HEADER) || id.is_none() {
            return StatusCode::BAD_REQUEST.into_response();
        }
        let params = request.get("params").unwrap_or(&Value::Null);
        let result = mcp::initialize_result(params);
        let version = result["protocolVersion"]
            .as_str()
            .expect("initialize result includes a version");
        let session_id = match create_session(&state, &actor, version).await {
            Ok(id) => id,
            Err(status) => return status.into_response(),
        };
        let mut response = Json(json!({"jsonrpc":"2.0", "id":id, "result":result})).into_response();
        response.headers_mut().insert(
            SESSION_HEADER,
            HeaderValue::from_str(&session_id.to_string()).expect("UUID header is valid"),
        );
        response
            .headers_mut()
            .insert("cache-control", HeaderValue::from_static("no-store"));
        return response;
    }
    let session = match session(&state, &actor, &headers).await {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    if method == Some("notifications/initialized") && id.is_none() {
        if sqlx::query("UPDATE mcp_transport_sessions SET initialized = TRUE WHERE id = $1")
            .bind(session.id)
            .execute(&state.control_pool)
            .await
            .is_err()
        {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        return StatusCode::ACCEPTED.into_response();
    }
    if !session.initialized {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if id.is_none() || method.is_none() {
        return StatusCode::ACCEPTED.into_response();
    }
    let work = async move {
        let id = id.unwrap_or(Value::Null);
        let result = mcp::dispatch_request(&state, &actor, request).await;
        match result {
            Ok(result) => json!({"jsonrpc":"2.0", "id":id, "result":result}),
            Err((code, message)) => rpc_error(id, code, message),
        }
    };
    let mut response = if header_text(&headers, "accept")
        .is_some_and(|value| value.trim_start().starts_with("text/event-stream"))
    {
        Sse::new(stream::once(async move {
            Ok::<_, Infallible>(
                Event::default()
                    .event("message")
                    .data(work.await.to_string()),
            )
        }))
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
    } else {
        Json(work.await).into_response()
    };
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
}

pub async fn terminate(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthenticatedPrincipal>,
    headers: HeaderMap,
    _uri: Uri,
) -> Response {
    let session = match session(&state, &actor, &headers).await {
        Ok(session) => session,
        Err(status) => return status.into_response(),
    };
    match sqlx::query("DELETE FROM mcp_transport_sessions WHERE id = $1")
        .bind(session.id)
        .execute(&state.control_pool)
        .await
    {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
}

pub async fn listen(
    State(state): State<AppState>,
    Extension(actor): Extension<AuthenticatedPrincipal>,
    headers: HeaderMap,
    _uri: Uri,
) -> Response {
    if !accepts(&headers, "text/event-stream") || headers.contains_key("last-event-id") {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let current_session = match session(&state, &actor, &headers).await {
        Ok(session) if session.initialized => session,
        Ok(_) => return StatusCode::BAD_REQUEST.into_response(),
        Err(status) => return status.into_response(),
    };
    // One GET stream lease per session, across replicas. Abrupt disconnects
    // release the slot by its short TTL; no unbounded tasks or channel queues.
    let claimed = sqlx::query("UPDATE mcp_transport_sessions SET stream_until = NOW() + INTERVAL '60 seconds' WHERE id = $1 AND (stream_until IS NULL OR stream_until <= NOW())")
        .bind(current_session.id).execute(&state.control_pool).await;
    match claimed {
        Ok(result) if result.rows_affected() == 1 => {}
        Ok(_) => return StatusCode::TOO_MANY_REQUESTS.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    }
    let token = Zeroizing::new(
        header_text(&headers, "authorization")
            .unwrap_or_default()
            .trim_start_matches("Bearer ")
            .to_owned(),
    );
    let events = stream::unfold(
        (state, actor, headers, token, 0_u8),
        |(state, actor, headers, token, step)| async move {
            if step >= 12 {
                return None;
            }
            if step > 0 {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            let authenticated = if token.starts_with("sgmcp_") {
                oauth::authenticate_access_token(
                    &state.control_pool,
                    &token,
                    &oauth::mcp_resource(&state.mcp_public_url, "sql"),
                )
                .await
                .is_ok()
            } else {
                auth::authenticate_access_token(&state, &token)
                    .await
                    .is_ok()
            };
            if !authenticated || session(&state, &actor, &headers).await.is_err() {
                return None;
            }
            Some((
                Ok::<_, Infallible>(Event::default().comment("keep-alive")),
                (state, actor, headers, token, step + 1),
            ))
        },
    );
    let mut response = Sse::new(events).into_response();
    response
        .headers_mut()
        .insert("x-accel-buffering", HeaderValue::from_static("no"));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_media_type_is_case_insensitive_and_allows_parameters() {
        let mut headers = HeaderMap::new();
        for value in [
            "application/json",
            "Application/JSON",
            "APPLICATION/JSON; charset=utf-8",
            "application/json ; charset=utf-8",
        ] {
            headers.insert("content-type", HeaderValue::from_static(value));
            assert!(
                json_content_type(&headers),
                "rejected valid content type: {value}"
            );
        }
        for value in [
            "text/plain",
            "application/jsonp",
            "application/json, text/plain",
            "",
        ] {
            headers.insert("content-type", HeaderValue::from_static(value));
            assert!(!json_content_type(&headers));
        }
        assert!(!json_content_type(&HeaderMap::new()));
        headers.insert(
            "content-type",
            HeaderValue::from_bytes(b"application/\xffjson").unwrap(),
        );
        assert!(!json_content_type(&headers));
    }

    #[test]
    fn protocol_header_distinguishes_absent_from_invalid_or_ambiguous_values() {
        let mut headers = HeaderMap::new();
        assert_eq!(protocol_version(&headers), Ok(None));
        headers.insert(VERSION_HEADER, HeaderValue::from_static("2025-11-25"));
        assert_eq!(protocol_version(&headers), Ok(Some("2025-11-25")));
        for value in ["", "2099-01-01", "2025-11-25, 2025-03-26"] {
            headers.insert(VERSION_HEADER, HeaderValue::from_static(value));
            assert_eq!(protocol_version(&headers), Err(StatusCode::BAD_REQUEST));
        }
        headers.insert(VERSION_HEADER, HeaderValue::from_bytes(b"\xff").unwrap());
        assert_eq!(protocol_version(&headers), Err(StatusCode::BAD_REQUEST));
        headers.insert(VERSION_HEADER, HeaderValue::from_static("2025-11-25"));
        headers.append(VERSION_HEADER, HeaderValue::from_static("2025-11-25"));
        assert_eq!(protocol_version(&headers), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn messages_require_jsonrpc_and_valid_request_or_response_shape() {
        for value in [
            json!({"method":"ping"}),
            json!({"jsonrpc":"2.0","method":5}),
            json!({"jsonrpc":"2.0","id":1}),
            json!({"jsonrpc":"2.0","id":null,"method":"ping"}),
        ] {
            assert!(!valid_message(value.as_object().unwrap()));
        }
        for value in [
            json!({"jsonrpc":"2.0","id":1,"method":"ping"}),
            json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
            json!({"jsonrpc":"2.0","id":1,"result":{}}),
        ] {
            assert!(valid_message(value.as_object().unwrap()));
        }
    }
}
