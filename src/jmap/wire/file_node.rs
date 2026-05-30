/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde::{Deserialize, Serialize};

use super::common::bool_or_true;
use super::{JmapId, UtcDate};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<JmapId>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<JmapId>,

    pub node_type: NodeType,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<JmapId>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<Vec<String>>,

    pub name: String,

    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,

    #[serde(with = "time::serde::rfc3339")]
    pub created: UtcDate,

    #[serde(
        default,
        with = "time::serde::rfc3339::option",
        skip_serializing_if = "Option::is_none"
    )]
    pub modified: Option<UtcDate>,

    #[serde(default = "default_true", deserialize_with = "bool_or_true")]
    pub is_subscribed: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    File,
    Directory,
    Symlink,
}

fn default_true() -> bool {
    true
}
