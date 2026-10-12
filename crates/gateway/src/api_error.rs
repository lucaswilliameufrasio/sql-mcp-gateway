use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Serialize;

#[derive(Debug, Clone, Copy)]
pub enum ApiError {
    Unauthorized,
    Forbidden,
    InvalidRequest,
    NotFound,
    Conflict,
    DependencyUnavailable,
}

#[derive(Serialize)]
struct ErrorBody {
    message: &'static str,
    error_code: &'static str,
}

impl ApiError {
    pub fn status(self) -> StatusCode {
        match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::DependencyUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    fn body(self) -> ErrorBody {
        let (message, error_code) = match self {
            Self::Unauthorized => ("Autenticação necessária.", "UNAUTHORIZED"),
            Self::Forbidden => ("Você não tem permissão para esta operação.", "FORBIDDEN"),
            Self::InvalidRequest => ("A solicitação é inválida.", "INVALID_REQUEST"),
            Self::NotFound => ("O recurso solicitado não foi encontrado.", "NOT_FOUND"),
            Self::Conflict => (
                "O recurso já existe ou conflita com o estado atual.",
                "CONFLICT",
            ),
            Self::DependencyUnavailable => (
                "Um serviço necessário está temporariamente indisponível.",
                "DEPENDENCY_UNAVAILABLE",
            ),
        };

        ErrorBody {
            message,
            error_code,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (self.status(), Json(self.body())).into_response()
    }
}
