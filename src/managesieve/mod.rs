/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

pub mod client;
pub mod command;
pub mod error;
pub mod name;
pub mod response;
pub mod retry;

pub use client::{ConnectMode, SieveClient};
pub use error::{NoError, SieveError};
pub use response::{
    Capabilities, ListedScript, ResponseBlock, Status, StatusLine, Token, parse_capabilities,
    parse_getscript, parse_listscripts, read_response, try_parse_status,
};
