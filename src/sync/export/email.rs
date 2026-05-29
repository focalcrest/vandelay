/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value, json};

use super::common::{jid, target_query_get};
use super::{Maps, Net, Plan, Uploader};
use crate::error::Error;
use crate::jmap::error::JmapError;
use crate::jmap::request::{Request, check_method_error, get_objects};
use crate::jmap::wire::JmapId;
use crate::logging::Logger;
use crate::sync::import_jmap::mapping::{EMAIL_SELECT, TargetResolver, row_to_email};
use crate::sync::keys::{EmailIndex, EmailKey, email_index, email_keys, index_from_json};
use crate::sync::{Context, TypeCounts};
use crate::types::ObjectType;

fn server_index(v: &Value) -> EmailIndex {
    let arr = |k: &str| {
        v.get(k)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.get("email").and_then(Value::as_str).map(str::to_owned))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    };
    let mids: Vec<String> = v
        .get("messageId")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    email_index(
        &mids,
        &arr("from"),
        v.get("subject").and_then(Value::as_str).unwrap_or(""),
        v.get("sentAt").and_then(Value::as_str).unwrap_or(""),
        &arr("to"),
    )
}

pub fn reconcile(
    ctx: &Context,
    net: &Net,
    maps: &mut Maps,
    counts: &mut TypeCounts,
    logger: &Logger,
) -> Result<Plan, Error> {
    let ty = ObjectType::Email;

    let target_min = target_query_get(net, ty, Some(&["messageId"])).map_err(Error::from)?;
    let mut indices: Vec<EmailIndex> = target_min.iter().map(server_index).collect();

    let fallback_ids: Vec<JmapId> = target_min
        .iter()
        .zip(indices.iter())
        .filter(|(_, i)| i.mids.is_empty())
        .filter_map(|(v, _)| jid(v).map(JmapId))
        .collect();
    if !fallback_ids.is_empty() {
        let got = get_objects::<Value>(
            &net.client,
            &net.api,
            &net.account,
            ty.jmap_name(),
            &fallback_ids,
            Some(&["messageId", "from", "subject", "sentAt", "to"]),
            &net.limits,
        )
        .map_err(Error::from)?;
        let by_id: HashMap<String, &Value> = got
            .list
            .iter()
            .filter_map(|v| jid(v).map(|i| (i, v)))
            .collect();
        for (v, slot) in target_min.iter().zip(indices.iter_mut()) {
            if let Some(full) = jid(v).and_then(|i| by_id.get(&i)) {
                *slot = server_index(full);
            }
        }
    }
    let target_keys: HashSet<EmailKey> = email_keys(&indices).into_iter().collect();

    let local: Vec<(i64, crate::sync::import_jmap::mapping::EmailRow)> = {
        let mut stmt = ctx
            .conn
            .prepare(EMAIL_SELECT)
            .map_err(|e| Error::Partial(e.to_string()))?;
        stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            Ok((id, row_to_email(row)))
        })
        .and_then(|m| m.collect::<Result<Vec<_>, _>>())
        .map_err(|e| Error::Partial(e.to_string()))?
        .into_iter()
        .map(|(id, r)| Ok((id, r.map_err(Error::from)?)))
        .collect::<Result<_, Error>>()?
    };

    let local_indices: Vec<EmailIndex> = local
        .iter()
        .map(|(_, r)| index_from_json(&r.message_match))
        .collect();
    let local_keys = email_keys(&local_indices);

    let mut uploader = Uploader::new(net, &ctx.conn);
    for (i, key) in local_keys.iter().enumerate() {
        if target_keys.contains(key) {
            counts.skipped += 1;
            continue;
        }
        let (local_id, row) = &local[i];
        let mut mids = Map::new();
        let mut all_resolved = true;
        for ml in &row.mailbox_locals {
            match maps.target(ObjectType::Mailbox, *ml) {
                Some(t) => {
                    mids.insert(t.0, Value::Bool(true));
                }
                None => {
                    all_resolved = false;
                    break;
                }
            }
        }
        if !all_resolved {
            logger.warn(&format!(
                "email local {local_id} skipped: mailbox not on target"
            ));
            counts.failed += 1;
            continue;
        }
        let blob = match uploader.upload_with(row.blob_local_id, "message/rfc822") {
            Ok(b) => b.0,
            Err(e) => {
                logger.warn(&format!("email blob upload failed: {e}"));
                counts.failed += 1;
                continue;
            }
        };
        let mut kw = Map::new();
        for k in &row.keywords {
            kw.insert(k.clone(), Value::Bool(true));
        }
        let item = json!({
            "blobId": blob,
            "mailboxIds": Value::Object(mids),
            "keywords": Value::Object(kw),
            "receivedAt": row.received_at,
        });
        send_import_chunk(net, &[(format!("e{local_id}"), item)], counts, logger);
    }

    Ok(Plan::default())
}

fn send_import_chunk(
    net: &Net,
    items: &[(String, Value)],
    counts: &mut TypeCounts,
    logger: &Logger,
) {
    if items.is_empty() {
        return;
    }
    if net.dry_run {
        counts.created += items.len() as u64;
        return;
    }
    let mut map = Map::new();
    for (k, v) in items {
        map.insert(k.clone(), v.clone());
    }
    let mut req = Request::new();
    req.call(
        "Email/import",
        json!({ "accountId": net.account, "emails": Value::Object(map) }),
        "i",
    );
    if req.fits(&net.limits).is_err() {
        resplit_or_fail(net, items, counts, logger);
        return;
    }
    match req.send(&net.client, &net.api) {
        Ok(resp) => match resp.first().and_then(|mr| {
            check_method_error(mr)?;
            Ok(mr)
        }) {
            Ok(mr) => absorb_import(mr, counts, logger),
            Err(JmapError::RequestTooLarge) => resplit_or_fail(net, items, counts, logger),
            Err(JmapError::Method { error_type, .. }) if error_type == "requestTooLarge" => {
                resplit_or_fail(net, items, counts, logger);
            }
            Err(e) => {
                logger.warn(&format!(
                    "Email/import method error ({} items): {e}",
                    items.len()
                ));
                counts.failed += items.len() as u64;
            }
        },
        Err(JmapError::RequestTooLarge) => resplit_or_fail(net, items, counts, logger),
        Err(e) => {
            logger.warn(&format!(
                "Email/import send failed ({} items): {e}",
                items.len()
            ));
            counts.failed += items.len() as u64;
        }
    }
}

fn resplit_or_fail(net: &Net, items: &[(String, Value)], counts: &mut TypeCounts, logger: &Logger) {
    if items.len() <= 1 {
        for (cid, _) in items {
            logger.warn(&format!(
                "Email/import {cid} exceeds maxSizeRequest alone; skipped"
            ));
        }
        counts.failed += items.len() as u64;
        return;
    }
    let mid = items.len() / 2;
    send_import_chunk(net, &items[..mid], counts, logger);
    send_import_chunk(net, &items[mid..], counts, logger);
}

fn absorb_import(mr: &crate::jmap::request::MethodCall, counts: &mut TypeCounts, logger: &Logger) {
    if let Some(created) = mr.args.get("created").and_then(Value::as_object) {
        counts.created += created.len() as u64;
    }
    if let Some(nc) = mr.args.get("notCreated").and_then(Value::as_object) {
        for (cid, err) in nc {
            let etype = err.get("type").and_then(Value::as_str).unwrap_or("");
            if etype == "alreadyExists" {
                counts.skipped += 1;
            } else {
                logger.warn(&format!("Email/import {cid} failed: {err}"));
                counts.failed += 1;
            }
        }
    }
}
