use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

#[derive(Debug)]
pub enum ApiError {
    BadRequest(String),
    Unauthorized,
    Forbidden,
    NotFound,
    Conflict(String),
    Internal,
}

impl ApiError {
    pub fn bad(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn unauthorized() -> Self {
        Self::Unauthorized
    }

    pub fn forbidden() -> Self {
        Self::Forbidden
    }

    pub fn not_found() -> Self {
        Self::NotFound
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn internal() -> Self {
        Self::Internal
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, message),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "missing or invalid token".into()),
            Self::Forbidden => (
                StatusCode::FORBIDDEN,
                "this token is not allowed to do that".into(),
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "not found".into()),
            Self::Conflict(message) => (StatusCode::CONFLICT, message),
            Self::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into()),
        };
        (status, Json(json!({ "error": message }))).into_response()
    }
}

#[derive(Debug)]
pub enum DbError {
    NotFound,
    Invalid(String),
    Conflict(String),
    Internal(String),
}

impl From<rusqlite::Error> for DbError {
    fn from(err: rusqlite::Error) -> Self {
        eprintln!("sqlite error: {err}");
        match &err {
            rusqlite::Error::SqliteFailure(code, message)
                if code.code == rusqlite::ErrorCode::ConstraintViolation =>
            {
                DbError::Conflict(constraint_message(message.as_deref()))
            }
            rusqlite::Error::QueryReturnedNoRows => DbError::NotFound,
            _ => DbError::Internal("database error".into()),
        }
    }
}

fn constraint_message(message: Option<&str>) -> String {
    let message = message.unwrap_or("");
    if message.contains("FOREIGN KEY") {
        "that refers to something that does not exist".into()
    } else if message.contains("UNIQUE") {
        "a record with that name already exists".into()
    } else {
        "that conflicts with existing data".into()
    }
}

impl From<DbError> for ApiError {
    fn from(err: DbError) -> Self {
        match err {
            DbError::NotFound => ApiError::not_found(),
            DbError::Invalid(message) => ApiError::bad(message),
            DbError::Conflict(message) => ApiError::conflict(message),
            DbError::Internal(message) => {
                eprintln!("internal error: {message}");
                ApiError::internal()
            }
        }
    }
}
