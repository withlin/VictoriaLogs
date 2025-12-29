use std::collections::HashMap;

use serde::Serialize;

use crate::errors::StorageError;

#[derive(Debug, Clone, Default)]
pub struct Request {
    pub path: String,
    pub query: HashMap<String, String>,
}

impl Request {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            query: HashMap::new(),
        }
    }

    pub fn query_u64(&self, key: &str) -> Option<u64> {
        self.query.get(key).and_then(|v| v.parse().ok())
    }
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub enum ResponseKind {
    Ok,
    Accepted,
    BadRequest,
    TooManyRequests,
    NotImplemented,
    InternalError,
}

impl ResponseKind {
    pub fn status(self) -> u16 {
        match self {
            ResponseKind::Ok => 200,
            ResponseKind::Accepted => 202,
            ResponseKind::BadRequest => 400,
            ResponseKind::TooManyRequests => 429,
            ResponseKind::NotImplemented => 501,
            ResponseKind::InternalError => 500,
        }
    }
}

impl Response {
    pub fn empty(kind: ResponseKind) -> Self {
        Self {
            status: kind.status(),
            content_type: None,
            body: None,
        }
    }

    pub fn json<T: Serialize>(kind: ResponseKind, value: &T) -> Self {
        let body = serde_json::to_string(value)
            .unwrap_or_else(|err| serde_json::json!({ "error": err.to_string() }).to_string());

        Self {
            status: kind.status(),
            content_type: Some("application/json".to_string()),
            body: Some(body),
        }
    }

    pub fn text(kind: ResponseKind, body: impl Into<String>) -> Self {
        Self {
            status: kind.status(),
            content_type: Some("text/plain".to_string()),
            body: Some(body.into()),
        }
    }

    pub fn error(err: StorageError) -> Self {
        let kind = match err.status_code() {
            400 => ResponseKind::BadRequest,
            429 => ResponseKind::TooManyRequests,
            501 => ResponseKind::NotImplemented,
            _ => ResponseKind::InternalError,
        };

        Self::text(kind, err.to_string())
    }
}
