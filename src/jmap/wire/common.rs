/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JmapId(pub String);

impl From<String> for JmapId {
    fn from(value: String) -> Self {
        JmapId(value)
    }
}

impl From<&str> for JmapId {
    fn from(value: &str) -> Self {
        JmapId(value.to_owned())
    }
}

impl std::fmt::Display for JmapId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub type UtcDate = time::OffsetDateTime;

pub type Date = time::OffsetDateTime;

pub fn bool_or_true<'de, D>(d: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<bool>::deserialize(d)?.unwrap_or(true))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailAddress {
    pub name: Option<String>,
    pub email: String,
}
