/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use crate::error::Error;

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("oauth error: {0}")]
    OAuth(String),

    #[error("graph http {status}: {body}")]
    HttpStatus { status: u16, body: String },

    #[error("graph auth failure: {0}")]
    Auth(String),

    #[error("graph transport error: {0}")]
    Transport(String),

    #[error("graph connect error: {0}")]
    Connect(String),

    #[error("retries exhausted: {0}")]
    RetriesExhausted(String),

    #[error("malformed graph response: {0}")]
    Malformed(String),

    #[error("graph item vanished")]
    Vanished,
}

impl From<GraphError> for Error {
    fn from(e: GraphError) -> Self {
        match e {
            GraphError::OAuth(m) => Error::Connection(format!("oauth: {m}")),
            GraphError::Auth(m) => Error::Connection(format!("auth: {m}")),
            GraphError::Connect(m) => Error::Connection(m),
            GraphError::Transport(m) => Error::Connection(m),
            GraphError::RetriesExhausted(m) => Error::Connection(m),
            GraphError::HttpStatus { status, body } => {
                Error::Connection(format!("http {status}: {body}"))
            }
            GraphError::Malformed(m) => Error::Partial(format!("malformed: {m}")),
            GraphError::Vanished => Error::Partial("graph item vanished".to_owned()),
        }
    }
}
