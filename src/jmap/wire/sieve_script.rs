/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde::{Deserialize, Serialize};

use super::JmapId;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SieveScript {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<JmapId>,

    #[serde(default)]
    pub name: Option<String>,

    pub blob_id: JmapId,

    #[serde(default)]
    pub is_active: bool,
}
