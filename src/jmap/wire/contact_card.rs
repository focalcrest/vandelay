/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::JmapId;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactCard {

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<JmapId>,

    pub address_book_ids: IndexMap<JmapId, bool>,

    #[serde(flatten)]
    pub rest: IndexMap<String, Value>,
}
