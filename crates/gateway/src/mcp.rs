use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header::CONTENT_TYPE},
    response::{IntoResponse, Response},
};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{
    auth::AuthenticatedPrincipal,
    domain::{AccessLevel, DatabaseEngine},
    sql_access::{QueryMode, QueryResponse, run_database_query},
    state::AppState,
};

pub async fn http_mcp(
    State(state): State<AppState>,
    axum::Extension(actor): axum::Extension<AuthenticatedPrincipal>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(';').next().is_some_and(|media_type| {
                media_type.trim().eq_ignore_ascii_case("application/json")
            })
        })
    {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    }
    if !headers
        .get("accept")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value.split(',').any(|media_type| {
                media_type.split(';').next().is_some_and(|media_type| {
                    media_type.trim().eq_ignore_ascii_case("application/json")
                })
            })
        })
    {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }
    if let Some(version) = headers
        .get("MCP-Protocol-Version")
        .and_then(|value| value.to_str().ok())
        && !supported_protocol_version(version)
    {
        return StatusCode::BAD_REQUEST.into_response();
    }

    let request = match serde_json::from_slice::<Value>(&body) {
        Ok(Value::Object(request)) => request,
        _ => {
            return json_rpc_error(
                Value::Null,
                -32600,
                "Invalid Request",
                StatusCode::BAD_REQUEST,
            );
        }
    };
    let is_notification = !request.contains_key("id");
    if is_notification {
        return StatusCode::ACCEPTED.into_response();
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let response = dispatch_request(&state, &actor, request).await;

    match response {
        Ok(result) => (
            StatusCode::OK,
            [(CONTENT_TYPE, "application/json")],
            axum::Json(json!({"jsonrpc": "2.0", "id": id, "result": result})),
        )
            .into_response(),
        Err((code, message)) => json_rpc_error(id, code, message, StatusCode::OK),
    }
}

pub async fn dispatch_stdio_line(
    state: &AppState,
    actor: &AuthenticatedPrincipal,
    line: &str,
) -> Option<Value> {
    let request = match serde_json::from_str::<Value>(line) {
        Ok(Value::Object(request)) => request,
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": {"code": -32600, "message": "Invalid Request"}
            }));
        }
    };
    let is_notification = !request.contains_key("id");
    if is_notification {
        return None;
    }
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let response = dispatch_request(state, actor, request).await;

    Some(match response {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err((code, message)) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": message}
        }),
    })
}

async fn dispatch_request(
    state: &AppState,
    actor: &AuthenticatedPrincipal,
    request: Map<String, Value>,
) -> Result<Value, (i64, &'static str)> {
    if request.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return Err((-32600, "Invalid Request"));
    }
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .ok_or((-32600, "Invalid Request"))?;
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

    match method {
        "initialize" => Ok(initialize_result(&params)),
        "notifications/initialized" | "notifications/cancelled" => Ok(Value::Null),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_definitions()})),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or((-32602, "Invalid params"))?;
            let arguments = params
                .get("arguments")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            let call_result = call_tool(state, actor, name, arguments).await;
            match call_result {
                Ok(output) => Ok(json!({
                    "content": [{"type": "text", "text": serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_owned())}],
                    "structuredContent": output,
                    "isError": false
                })),
                Err(message) => Ok(json!({
                    "content": [{"type": "text", "text": message}],
                    "isError": true
                })),
            }
        }
        _ => Err((-32601, "Method not found")),
    }
}

fn initialize_result(params: &Value) -> Value {
    let requested_version = params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .unwrap_or("2026-07-28");
    let protocol_version = if supported_protocol_version(requested_version) {
        requested_version
    } else {
        "2026-07-28"
    };

    json!({
        "protocolVersion": protocol_version,
        "capabilities": {"tools": {"listChanged": false}},
        "serverInfo": {"name": "sql-mcp-gateway", "version": env!("CARGO_PKG_VERSION")},
        "instructions": "Acesso SQL autorizado por usuário e banco. read_query aceita apenas SELECT; write_query aceita INSERT, UPDATE e DELETE, limitado por permissões nativas do banco."
    })
}

fn supported_protocol_version(version: &str) -> bool {
    matches!(
        version,
        "2024-11-05" | "2025-03-26" | "2025-06-18" | "2025-11-25" | "2026-07-28"
    )
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_databases",
            "description": "Lista os bancos explicitamente concedidos a esta identidade.",
            "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false}
        }),
        json!({
            "name": "describe_database",
            "description": "Lista tabelas e colunas visíveis no banco concedido.",
            "inputSchema": {
                "type": "object",
                "properties": {"database_id": {"type": "string", "format": "uuid"}},
                "required": ["database_id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "read_query",
            "description": "Executa uma única consulta SELECT em um banco com concessão de leitura. Retorna no máximo o limite configurado de linhas.",
            "inputSchema": query_tool_schema()
        }),
        json!({
            "name": "write_query",
            "description": "Executa uma única instrução INSERT, UPDATE ou DELETE em um banco com concessão de escrita. DDL, múltiplas instruções e RETURNING não são aceitos.",
            "inputSchema": query_tool_schema()
        }),
    ]
}

fn query_tool_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "database_id": {"type": "string", "format": "uuid"},
            "sql": {"type": "string", "minLength": 1, "maxLength": 65536}
        },
        "required": ["database_id", "sql"],
        "additionalProperties": false
    })
}

async fn call_tool(
    state: &AppState,
    actor: &AuthenticatedPrincipal,
    name: &str,
    arguments: Map<String, Value>,
) -> Result<Value, &'static str> {
    match name {
        "list_databases" => list_database_tool(state, actor).await,
        "describe_database" => {
            let database_id = required_database_id(&arguments)?;
            let visible = list_database_tool(state, actor).await?;
            let database_id_text = database_id.to_string();
            if !visible.as_array().is_some_and(|databases| {
                databases.iter().any(|db| {
                    db.get("id").and_then(Value::as_str) == Some(database_id_text.as_str())
                })
            }) {
                return Err("Acesso a este banco não foi concedido.");
            }
            let metadata = sqlx::query_as::<_, DescribeTargetRow>(
                "SELECT d.id, d.schema_name FROM database_targets d \
                 JOIN principal_database_grants g ON g.database_id = d.id \
                 WHERE d.id = $1 AND g.principal_id = $2 AND d.enabled = TRUE",
            )
            .bind(database_id)
            .bind(actor.0.id)
            .fetch_optional(&state.control_pool)
            .await
            .map_err(|_| "Não foi possível acessar o banco agora.")?
            .ok_or("Acesso a este banco não foi concedido.")?;
            let escaped_schema = metadata.schema_name.replace('\'', "''");
            let query = format!(
                "SELECT table_name, column_name, data_type FROM information_schema.columns WHERE table_schema = '{escaped_schema}' ORDER BY table_name, ordinal_position"
            );
            run_database_query(state, actor, database_id, &query, QueryMode::Read)
                .await
                .map(query_response_to_value)
                .map_err(query_error_message)
        }
        "read_query" | "write_query" => {
            let database_id = required_database_id(&arguments)?;
            let sql = arguments
                .get("sql")
                .and_then(Value::as_str)
                .ok_or("O campo sql é obrigatório.")?;
            let mode = if name == "read_query" {
                QueryMode::Read
            } else {
                QueryMode::Write
            };
            run_database_query(state, actor, database_id, sql, mode)
                .await
                .map(query_response_to_value)
                .map_err(query_error_message)
        }
        _ => Err("Ferramenta MCP desconhecida."),
    }
}

#[derive(sqlx::FromRow)]
struct DescribeTargetRow {
    schema_name: String,
}

fn required_database_id(arguments: &Map<String, Value>) -> Result<Uuid, &'static str> {
    arguments
        .get("database_id")
        .and_then(Value::as_str)
        .ok_or("O campo database_id é obrigatório.")?
        .parse()
        .map_err(|_| "O campo database_id deve ser um UUID válido.")
}

async fn list_database_tool(
    state: &AppState,
    actor: &AuthenticatedPrincipal,
) -> Result<Value, &'static str> {
    #[derive(sqlx::FromRow)]
    struct DatabaseRow {
        id: Uuid,
        name: String,
        engine: DatabaseEngine,
        access_level: AccessLevel,
    }

    let rows = sqlx::query_as::<_, DatabaseRow>(
        "SELECT d.id, d.name, d.engine, g.access_level FROM database_targets d \
         JOIN principal_database_grants g ON g.database_id = d.id \
         WHERE g.principal_id = $1 AND d.enabled = TRUE ORDER BY d.name",
    )
    .bind(actor.0.id)
    .fetch_all(&state.control_pool)
    .await
    .map_err(|_| "Não foi possível listar os bancos agora.")?;

    Ok(Value::Array(
        rows.into_iter()
            .map(|row| {
                json!({
                    "id": row.id,
                    "name": row.name,
                    "engine": match row.engine {
                        DatabaseEngine::Postgresql => "postgresql",
                        DatabaseEngine::Mysql => "mysql",
                    },
                    "access_level": match row.access_level {
                        AccessLevel::Read => "read",
                        AccessLevel::Write => "write",
                    }
                })
            })
            .collect(),
    ))
}

fn query_response_to_value(mut response: QueryResponse) -> Value {
    let mut encoded_rows = Vec::with_capacity(response.rows.len());
    for row in response.rows.drain(..) {
        encoded_rows.push(limit_json_value(row));
    }
    json!({
        "columns": response.columns,
        "rows": encoded_rows,
        "affected_rows": response.affected_rows,
        "truncated": response.truncated
    })
}

fn limit_json_value(value: Value) -> Value {
    match value {
        Value::String(value) if value.len() > 16_384 => {
            let boundary = value
                .char_indices()
                .take_while(|(index, _)| *index < 16_384)
                .last()
                .map(|(index, character)| index + character.len_utf8())
                .unwrap_or_default();
            Value::String(format!("{}…[truncated]", &value[..boundary]))
        }
        Value::Array(values) => Value::Array(values.into_iter().map(limit_json_value).collect()),
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| (key, limit_json_value(value)))
                .collect(),
        ),
        other => other,
    }
}

fn query_error_message(error: crate::sql_access::QueryError) -> &'static str {
    match error {
        crate::sql_access::QueryError::InvalidQuery => {
            "A consulta não é permitida por esta ferramenta."
        }
        crate::sql_access::QueryError::Denied => {
            "Acesso a este banco ou operação não foi concedido."
        }
        crate::sql_access::QueryError::DatabaseFailure => "A operação no banco falhou.",
        crate::sql_access::QueryError::TimedOut => "A operação excedeu o limite de tempo.",
        crate::sql_access::QueryError::BrokerUnavailable => {
            "O broker de credenciais está indisponível."
        }
    }
}

fn json_rpc_error(id: Value, code: i64, message: &str, status: StatusCode) -> Response {
    (
        status,
        [("content-type", "application/json")],
        axum::Json(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": {"code": code, "message": message}
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{initialize_result, supported_protocol_version, tool_definitions};

    #[test]
    fn advertises_supported_protocol_and_tools_without_unbounded_sql_capabilities() {
        assert!(supported_protocol_version("2026-07-28"));
        assert!(!supported_protocol_version("2099-01-01"));

        let initialized = initialize_result(&json!({"protocolVersion": "2025-11-25"}));
        assert_eq!(initialized["protocolVersion"], "2025-11-25");

        let names = tool_definitions()
            .into_iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                "list_databases",
                "describe_database",
                "read_query",
                "write_query"
            ]
        );
    }
}
