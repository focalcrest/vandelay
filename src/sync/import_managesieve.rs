/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

pub mod coordinator;
pub mod reconcile;

pub use coordinator::{ManageSieveAuth, ManageSieveImportConfig, run};
