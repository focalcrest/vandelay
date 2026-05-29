/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

pub mod cli;
pub mod dav;
pub mod db;
pub mod error;
pub mod exchange;
pub mod exchange_ews;
pub mod exchange_graph;
pub mod imap;
pub mod inspect;
pub mod jmap;
pub mod logging;
pub mod managesieve;
pub mod secret;
pub mod sync;
pub mod types;

use std::sync::Once;

static CRYPTO_INIT: Once = Once::new();

pub fn install_default_crypto_provider() {
    CRYPTO_INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}
