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
pub struct Calendar {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<JmapId>,

    pub name: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,

    #[serde(default)]
    pub sort_order: u32,

    #[serde(default = "default_true")]
    pub is_subscribed: bool,

    #[serde(default = "default_true")]
    pub is_visible: bool,

    #[serde(default)]
    pub is_default: bool,

    #[serde(default = "default_include_in_availability")]
    pub include_in_availability: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_alerts_with_time: Option<IndexMap<String, Value>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_alerts_without_time: Option<IndexMap<String, Value>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_zone: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_include_in_availability() -> String {
    "all".to_owned()
}
