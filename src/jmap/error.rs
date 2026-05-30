/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use crate::error::Error;
use crate::jmap::blob::BlobWalkError;

#[derive(Debug, thiserror::Error)]
pub enum JmapError {
    #[error("transport failure: {0}")]
    Transport(String),

    #[error("connection failure: {0}")]
    Connect(String),

    #[error("authentication rejected: {0}")]
    Auth(String),

    #[error("http status {status}: {body}")]
    HttpStatus { status: u16, body: String },

    #[error("retries exhausted: {0}")]
    RetriesExhausted(String),

    #[error("request too large")]
    RequestTooLarge,

    #[error("single object exceeds the server size limit and cannot be split: {0}")]
    SingleObjectTooLarge(String),

    #[error("query anchor not found")]
    AnchorNotFound,

    #[error("server cannot calculate changes from the stored state")]
    CannotCalculateChanges,

    #[error("server does not implement the requested method")]
    UnknownMethod,

    #[error("jmap method error in call {call_id}: {error_type}{}", .description.as_deref().map(|d| format!(" ({d})")).unwrap_or_default())]
    Method {
        call_id: String,
        error_type: String,
        description: Option<String>,
    },

    #[error("malformed jmap response: {0}")]
    Malformed(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("blob walk error: {0}")]
    Blob(#[from] BlobWalkError),
}

impl JmapError {
    pub fn malformed(context: impl Into<String>) -> JmapError {
        JmapError::Malformed(context.into())
    }
}

impl From<JmapError> for Error {
    fn from(value: JmapError) -> Self {
        match value {
            JmapError::Connect(m) | JmapError::Transport(m) => Error::Connection(m),
            JmapError::Auth(m) => Error::Connection(format!("authentication rejected: {m}")),
            other => Error::Connection(other.to_string()),
        }
    }
}
