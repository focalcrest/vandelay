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
pub struct CalendarEvent {

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<JmapId>,

    pub calendar_ids: IndexMap<JmapId, bool>,

    #[serde(default)]
    pub is_draft: bool,

    #[serde(default)]
    pub use_default_alerts: bool,

    #[serde(flatten)]
    pub rest: IndexMap<String, Value>,
}
