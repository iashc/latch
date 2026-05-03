use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use serde_json::{Map, Value};

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
    details: Option<Map<String, Value>>,
}

impl ApiError {
    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid_request", message)
    }

    pub fn bookmark_not_found() -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "bookmark_not_found",
            "Bookmark not found",
        )
    }

    pub fn duplicate_url(url: &str) -> Self {
        let mut error = Self::new(
            StatusCode::CONFLICT,
            "duplicate_url",
            format!("Bookmark already exists for URL: {url}"),
        );
        error = error.with_detail("url", Value::String(url.to_owned()));
        error
    }

    pub fn import_invalid_item(
        item_index: usize,
        field: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "import_invalid_item", message)
            .with_detail("item_index", Value::from(item_index))
            .with_detail("field", Value::String(field.to_owned()))
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", message)
    }

    pub fn with_detail(mut self, key: &'static str, value: Value) -> Self {
        let details = self.details.get_or_insert_with(Map::new);
        details.insert(key.to_owned(), value);
        self
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            details: None,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

#[derive(Debug, Serialize)]
struct ErrorResponse<'a> {
    error: ErrorBody<'a>,
}

#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<&'a Map<String, Value>>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = ErrorResponse {
            error: ErrorBody {
                code: self.code,
                message: &self.message,
                details: self.details.as_ref(),
            },
        };

        (self.status, Json(body)).into_response()
    }
}
