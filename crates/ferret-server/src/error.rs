use axum::{
    Json,
    extract::rejection::JsonRejection,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// A rejected request, answered with `{"error": "<message>"}`.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn unauthorized(message: &str) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.to_owned(),
        }
    }

    /// Authenticated, but not allowed to do this.
    pub fn forbidden(message: &str) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.to_owned(),
        }
    }
}

/// axum answers a malformed body or a wrong content type in plain text (400, 415 or 422).
impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self {
            status: rejection.status(),
            message: rejection.body_text(),
        }
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(error: sqlx::Error) -> Self {
        tracing::error!(%error, "database error");
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: "internal error".to_owned(),
        }
    }
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a str,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorBody {
            error: &self.message,
        };
        (self.status, Json(body)).into_response()
    }
}
