/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::io;

use thiserror::Error;

pub type ContainerResult<T> = Result<T, ContainerError>;

#[derive(Debug, Error)]
pub enum ContainerError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("testcontainers: {0}")]
    Testcontainers(#[from] testcontainers::TestcontainersError),
    #[error("ureq: {0}")]
    Ureq(#[from] Box<ureq::Error>),
    #[error("utf8: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
    #[error("env: {0}")]
    Env(String),
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("seed: {0}")]
    Seed(String),
    #[error("resource: {0}")]
    Resource(String),
}

impl From<ureq::Error> for ContainerError {
    fn from(e: ureq::Error) -> Self {
        ContainerError::Ureq(Box::new(e))
    }
}
