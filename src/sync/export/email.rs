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
use crate::sync::import_jmap::mapping::{EMAIL_SELECT, EmailRow, TargetResolver, row_to_email};
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

    let local: Vec<(i64, EmailRow)> = {
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
    // Upload each blob, then accumulate the resulting import descriptors and
    // flush them to the target in batches: a single Email/import call carries up
    // to `maxObjectsInSet` emails, cutting per-message round-trips on bulk
    // migrations. (Blob uploads themselves remain one-at-a-time.)
    let chunk = net.limits.max_objects_in_set.max(1) as usize;
    let mut batch: Vec<Pending> = Vec::with_capacity(chunk);
    for (i, key) in local_keys.iter().enumerate() {
        if target_keys.contains(key) {
            counts.skipped += 1;
            continue;
        }
        let (local_id, row) = &local[i];
        if let Some(pending) = prepare_import(net, &mut uploader, maps, *local_id, row, counts, logger)
        {
            batch.push(pending);
            if batch.len() >= chunk {
                flush_import_batch(net, &mut uploader, maps, &mut batch, counts, logger, false);
            }
        }
    }
    flush_import_batch(net, &mut uploader, maps, &mut batch, counts, logger, false);

    Ok(Plan::default())
}

fn build_mailbox_ids(row: &EmailRow, maps: &Maps) -> Option<Map<String, Value>> {
    let mut mids = Map::new();
    for ml in &row.mailbox_locals {
        let t = maps.target(ObjectType::Mailbox, *ml)?;
        mids.insert(t.0, Value::Bool(true));
    }
    Some(mids)
}

fn build_keywords(row: &EmailRow) -> Map<String, Value> {
    let mut kw = Map::new();
    for k in &row.keywords {
        kw.insert(k.clone(), Value::Bool(true));
    }
    kw
}

fn blob_hint(uploader: &Uploader, row: &EmailRow) -> String {
    let idx = index_from_json(&row.message_match);
    let mut s = match idx.mids.first() {
        Some(mid) => format!("message-id <{mid}>"),
        None => "no message-id".to_owned(),
    };
    if let Some(len) = uploader.blob_len(row.blob_local_id) {
        use std::fmt::Write;
        let _ = write!(s, ", {}", crate::inspect::format_bytes(len));
    }
    s
}

fn size_note(e: &JmapError) -> &'static str {
    if matches!(
        e,
        JmapError::RequestTooLarge | JmapError::SingleObjectTooLarge(_)
    ) {
        "; exceeds the target server size limit, so this message is skipped and re-running will not migrate it"
    } else {
        ""
    }
}

fn import_item(
    blob: String,
    mids: Map<String, Value>,
    kw: Map<String, Value>,
    received_at: &str,
) -> Value {
    json!({
        "blobId": blob,
        "mailboxIds": Value::Object(mids),
        "keywords": Value::Object(kw),
        "receivedAt": received_at,
    })
}

/// One email that has had its blob uploaded and is ready to be imported. Held
/// in a batch so many can be created in a single Email/import request.
struct Pending<'a> {
    cid: String,
    row: &'a EmailRow,
    item: Value,
}

/// Per-creation-id outcome of an Email/import call.
enum ImportOutcome {
    Created,
    Skipped,
    BlobNotFound,
    TooLarge,
    Failed(String),
}

/// Upload the blob and build the import descriptor for one email. Returns
/// `None` (having already updated `counts`/logged) when the email cannot be
/// staged for import, or in dry-run mode where nothing is sent.
fn prepare_import<'a>(
    net: &Net,
    uploader: &mut Uploader,
    maps: &Maps,
    local_id: i64,
    row: &'a EmailRow,
    counts: &mut TypeCounts,
    logger: &Logger,
) -> Option<Pending<'a>> {
    let cid = format!("e{local_id}");
    let mids = match build_mailbox_ids(row, maps) {
        Some(m) => m,
        None => {
            logger.warn(&format!(
                "Email/import {cid} ({}) skipped: mailbox not on target",
                blob_hint(uploader, row)
            ));
            counts.failed += 1;
            return None;
        }
    };
    let blob = match uploader.upload_with(row.blob_local_id, "message/rfc822") {
        Ok(b) => b.0,
        Err(e) => {
            logger.warn(&format!(
                "Email/import {cid} ({}) blob upload failed: {e}{}",
                blob_hint(uploader, row),
                size_note(&e)
            ));
            counts.failed += 1;
            return None;
        }
    };
    if net.dry_run {
        counts.created += 1;
        return None;
    }
    let item = import_item(blob, mids, build_keywords(row), &row.received_at);
    Some(Pending { cid, row, item })
}

/// Send one batch of prepared emails, tally the per-message outcomes, and, on
/// the first pass, re-upload + re-issue any that failed with `blobNotFound`.
/// Drains `batch`.
fn flush_import_batch(
    net: &Net,
    uploader: &mut Uploader,
    maps: &Maps,
    batch: &mut Vec<Pending>,
    counts: &mut TypeCounts,
    logger: &Logger,
    is_retry: bool,
) {
    if batch.is_empty() {
        return;
    }
    let outcomes = match send_import_batch(net, batch) {
        Ok(o) => o,
        Err(e) => {
            // A transport/method-level failure fails the whole batch; count and
            // log each message so totals stay accurate.
            for p in batch.iter() {
                logger.warn(&format!(
                    "Email/import {} ({}) send failed{}: {e}{}",
                    p.cid,
                    blob_hint(uploader, p.row),
                    if is_retry { " after blob re-upload" } else { "" },
                    size_note(&e)
                ));
                counts.failed += 1;
            }
            batch.clear();
            return;
        }
    };

    let mut to_retry: Vec<Pending> = Vec::new();
    for p in batch.drain(..) {
        match outcomes.get(&p.cid) {
            Some(ImportOutcome::Created) => counts.created += 1,
            Some(ImportOutcome::Skipped) => counts.skipped += 1,
            Some(ImportOutcome::BlobNotFound) if !is_retry => to_retry.push(p),
            Some(ImportOutcome::BlobNotFound) => {
                logger.warn(&format!(
                    "Email/import {} ({}) failed after blob re-upload: blobNotFound",
                    p.cid,
                    blob_hint(uploader, p.row)
                ));
                counts.failed += 1;
            }
            Some(ImportOutcome::TooLarge) => {
                logger.warn(&format!(
                    "Email/import {} ({}) failed: exceeds the target server size limit, so this message is skipped and re-running will not migrate it",
                    p.cid,
                    blob_hint(uploader, p.row)
                ));
                counts.failed += 1;
            }
            Some(ImportOutcome::Failed(detail)) => {
                logger.warn(&format!(
                    "Email/import {} ({}) failed{}: {detail}",
                    p.cid,
                    blob_hint(uploader, p.row),
                    if is_retry { " after blob re-upload" } else { "" }
                ));
                counts.failed += 1;
            }
            None => {
                logger.warn(&format!(
                    "Email/import {} ({}) failed: no result returned",
                    p.cid,
                    blob_hint(uploader, p.row)
                ));
                counts.failed += 1;
            }
        }
    }

    if to_retry.is_empty() {
        return;
    }

    // Re-upload the blobs the server said it could not find, rebuild the import
    // descriptors with the fresh blob ids, and re-issue just that subset once.
    let mut retry_batch: Vec<Pending> = Vec::with_capacity(to_retry.len());
    for mut p in to_retry {
        uploader.invalidate(p.row.blob_local_id);
        let blob = match uploader.upload_with(p.row.blob_local_id, "message/rfc822") {
            Ok(b) => b.0,
            Err(e) => {
                logger.warn(&format!(
                    "Email/import {} ({}) blob re-upload failed: {e}{}",
                    p.cid,
                    blob_hint(uploader, p.row),
                    size_note(&e)
                ));
                counts.failed += 1;
                continue;
            }
        };
        let mids = match build_mailbox_ids(p.row, maps) {
            Some(m) => m,
            None => {
                logger.warn(&format!(
                    "Email/import {} ({}) skipped: mailbox not on target",
                    p.cid,
                    blob_hint(uploader, p.row)
                ));
                counts.failed += 1;
                continue;
            }
        };
        p.item = import_item(blob, mids, build_keywords(p.row), &p.row.received_at);
        retry_batch.push(p);
    }
    flush_import_batch(net, uploader, maps, &mut retry_batch, counts, logger, true);
}

/// Issue a single Email/import for every email in `batch`, returning the
/// per-creation-id outcome. On `requestTooLarge` the batch is split in half and
/// retried recursively (mirroring `set_send`); a lone email that is still too
/// large is reported as `TooLarge` for that cid rather than aborting the run.
/// Genuine transport/method errors propagate to the caller.
fn send_import_batch(
    net: &Net,
    batch: &[Pending],
) -> Result<HashMap<String, ImportOutcome>, JmapError> {
    if batch.is_empty() {
        return Ok(HashMap::new());
    }
    let mut emails = Map::new();
    for p in batch {
        emails.insert(p.cid.clone(), p.item.clone());
    }
    let mut req = Request::new();
    req.call(
        "Email/import",
        json!({ "accountId": net.account, "emails": Value::Object(emails) }),
        "i",
    );

    if req.fits(&net.limits).is_err() {
        if batch.len() <= 1 {
            let mut out = HashMap::new();
            out.insert(batch[0].cid.clone(), ImportOutcome::TooLarge);
            return Ok(out);
        }
        let mid = batch.len() / 2;
        let mut left = send_import_batch(net, &batch[..mid])?;
        let right = send_import_batch(net, &batch[mid..])?;
        left.extend(right);
        return Ok(left);
    }

    let resp = req.send(&net.client, &net.api)?;
    let mr = resp.first()?;
    check_method_error(mr)?;
    let not_created = mr.args.get("notCreated").and_then(Value::as_object);
    let created = mr.args.get("created").and_then(Value::as_object);

    let mut out = HashMap::with_capacity(batch.len());
    for p in batch {
        if let Some(err) = not_created.and_then(|nc| nc.get(&p.cid)) {
            let error_type = err.get("type").and_then(Value::as_str).unwrap_or("");
            let outcome = match error_type {
                "alreadyExists" => ImportOutcome::Skipped,
                "blobNotFound" => ImportOutcome::BlobNotFound,
                _ => ImportOutcome::Failed(err.to_string()),
            };
            out.insert(p.cid.clone(), outcome);
        } else if created.is_some_and(|c| c.contains_key(&p.cid)) {
            out.insert(p.cid.clone(), ImportOutcome::Created);
        } else {
            out.insert(
                p.cid.clone(),
                ImportOutcome::Failed(format!(
                    "Email/import returned neither created nor notCreated for {}",
                    p.cid
                )),
            );
        }
    }
    Ok(out)
}
