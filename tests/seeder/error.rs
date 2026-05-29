/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

#[derive(Debug, thiserror::Error)]
pub enum SeedError {
    #[error("http error: {0}")]
    Http(String),

    #[error("jmap method error ({method}): {detail}")]
    Method { method: String, detail: String },

    #[error("unexpected jmap response: {0}")]
    Shape(String),

    #[error("resource error: {0}")]
    Resource(String),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type SeedResult<T> = Result<T, SeedError>;
