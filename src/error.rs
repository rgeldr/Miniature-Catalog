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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    #[test]
    fn api_error_responses_use_the_status_and_message_for_each_variant() {
        let cases = [
            (ApiError::unauthorized(), 401, "missing or invalid token"),
            (
                ApiError::forbidden(),
                403,
                "this token is not allowed to do that",
            ),
            (ApiError::not_found(), 404, "not found"),
            (ApiError::internal(), 500, "internal error"),
            (ApiError::bad("name is required"), 400, "name is required"),
            (ApiError::conflict("already exists"), 409, "already exists"),
        ];
        for (error, status, message) in cases {
            let response = error.into_response();
            assert_eq!(response.status().as_u16(), status);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX);
            let body = futures_executor(body);
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["error"], message);
        }
        assert!(
            constraint_message(Some("FOREIGN KEY constraint failed")).contains("does not exist")
        );
        assert!(
            constraint_message(Some("UNIQUE constraint failed: tags.name"))
                .contains("already exists")
        );
        assert!(constraint_message(None).contains("conflicts"));
        let mapped = ApiError::from(DbError::NotFound);
        assert_eq!(mapped.into_response().status().as_u16(), 404);
    }

    fn futures_executor(
        future: impl std::future::Future<Output = Result<axum::body::Bytes, axum::Error>>,
    ) -> axum::body::Bytes {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(future).unwrap()
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
