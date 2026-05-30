/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde::{Deserialize, Serialize};

use super::{EmailAddress, JmapId};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<JmapId>,

    #[serde(default)]
    pub name: String,

    pub email: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Vec<EmailAddress>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bcc: Option<Vec<EmailAddress>>,

    #[serde(default)]
    pub text_signature: String,

    #[serde(default)]
    pub html_signature: String,
}
