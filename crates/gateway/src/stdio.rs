use futures_util::StreamExt;
use tokio::io::{AsyncWriteExt, stdin, stdout};
use tokio_util::codec::{FramedRead, LinesCodec};
use zeroize::Zeroizing;

use crate::{auth::authenticate_access_token, mcp::dispatch_stdio_line, state::AppState};

const MAX_LINE_BYTES: usize = 256 * 1024;

pub async fn run(state: AppState) {
    let access_token = match std::env::var("SQL_MCP_ACCESS_TOKEN") {
        Ok(token) if !token.trim().is_empty() => Zeroizing::new(token),
        _ => {
            tracing::error!("SQL_MCP_ACCESS_TOKEN is required in stdio mode");
            return;
        }
    };
    let mut lines = FramedRead::new(stdin(), LinesCodec::new_with_max_length(MAX_LINE_BYTES));
    let mut output = stdout();

    while let Some(line_result) = lines.next().await {
        let line = match line_result {
            Ok(line) => line,
            Err(_) => {
                tracing::warn!("stdio input line exceeded the size limit or could not be decoded");
                let _ = output
                    .write_all(b"{\"jsonrpc\":\"2.0\",\"id\":null,\"error\":{\"code\":-32600,\"message\":\"Invalid Request\"}}\n")
                    .await;
                continue;
            }
        };

        let identity = authenticate_access_token(&state, access_token.as_str()).await;
        let response = match identity {
            Ok(actor) => dispatch_stdio_line(&state, &actor, &line).await,
            Err(_) => stdio_auth_error(&line),
        };
        if let Some(response) = response {
            let encoded = match serde_json::to_vec(&response) {
                Ok(encoded) => encoded,
                Err(_) => {
                    tracing::error!("stdio response serialization failed");
                    break;
                }
            };
            if output.write_all(&encoded).await.is_err()
                || output.write_all(b"\n").await.is_err()
                || output.flush().await.is_err()
            {
                tracing::error!("stdio output stream failed");
                break;
            }
        }
    }
}

fn stdio_auth_error(line: &str) -> Option<serde_json::Value> {
    let request = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let object = request.as_object()?;
    if !object.contains_key("id") {
        return None;
    }

    Some(serde_json::json!({
        "jsonrpc": "2.0",
        "id": object.get("id").cloned().unwrap_or(serde_json::Value::Null),
        "error": {"code": -32001, "message": "Authentication required"}
    }))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::stdio_auth_error;

    #[test]
    fn authentication_error_preserves_request_id_but_never_echoes_credentials() {
        let line =
            r#"{"jsonrpc":"2.0","id":7,"method":"tools/list","access_token":"secret-test-value"}"#;
        let response = stdio_auth_error(line).expect("requests with an id receive a response");

        assert_eq!(response["id"], json!(7));
        assert_eq!(response["error"]["code"], json!(-32001));
        assert!(!response.to_string().contains("secret-test-value"));
    }

    #[test]
    fn notifications_do_not_receive_auth_errors() {
        assert!(
            stdio_auth_error(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).is_none()
        );
    }
}
