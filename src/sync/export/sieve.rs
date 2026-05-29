/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;

use serde_json::{Value, json};

use super::common::{create_batch, jid, target_get_all};
use super::{Maps, Net, Plan, Uploader};
use crate::db;
use crate::error::Error;
use crate::jmap::blobxfer;
use crate::jmap::request::Request;
use crate::logging::Logger;
use crate::sync::import_jmap::mapping::{SIEVE_SELECT, row_to_sieve_script};
use crate::sync::keys::blake3_bytes;
use crate::sync::{Context, TypeCounts};
use crate::types::ObjectType;

pub fn reconcile(
    ctx: &Context,
    net: &Net,
    _maps: &mut Maps,
    counts: &mut TypeCounts,
    logger: &Logger,
) -> Result<Plan, Error> {
    let ty = ObjectType::SieveScript;
    let targets = target_get_all(net, ty).map_err(Error::from)?;

    let mut target_by_key: HashMap<[u8; 32], String> = HashMap::new();
    for t in &targets {
        let (Some(id), Some(blob)) = (jid(t), t.get("blobId").and_then(Value::as_str)) else {
            continue;
        };
        let bytes = blobxfer::download_bytes(
            &net.client,
            &net.session,
            &net.account,
            blob,
            "application/sieve",
            "script",
        )
        .map_err(Error::from)?;
        target_by_key.insert(blake3_bytes(&bytes), id);
    }

    let locals: Vec<(i64, Option<String>, bool, i64)> = {
        let mut stmt = ctx
            .conn
            .prepare(SIEVE_SELECT)
            .map_err(|e| Error::Partial(e.to_string()))?;
        stmt.query_map([], |row| {
            let sr = row_to_sieve_script(row);
            Ok((row.get::<_, i64>(0)?, sr))
        })
        .and_then(|m| m.collect::<Result<Vec<_>, _>>())
        .map_err(|e| Error::Partial(e.to_string()))?
        .into_iter()
        .map(|(id, sr)| {
            let sr = sr.map_err(Error::from)?;
            Ok((id, sr.name, sr.is_active, sr.blob_local_id))
        })
        .collect::<Result<_, Error>>()?
    };

    let mut active_target: Option<String> = None;
    let mut deactivate = false;
    let mut uploader = Uploader::new(net, &ctx.conn);

    for (local, name, is_active, blob_local) in &locals {
        let bytes = db::blobs::blob_bytes(&ctx.conn, *blob_local)
            .map_err(|e| Error::Partial(e.to_string()))?
            .ok_or_else(|| Error::Partial("sieve blob missing".to_owned()))?;
        let key = blake3_bytes(&bytes);
        let target_id = if let Some(id) = target_by_key.get(&key) {
            counts.skipped += 1;
            id.clone()
        } else {
            let blob_id = uploader
                .upload_with(*blob_local, "application/sieve")
                .map_err(Error::from)?;
            let mut obj = serde_json::Map::new();
            if let Some(n) = name {
                obj.insert("name".to_owned(), Value::String(n.clone()));
            }
            obj.insert("blobId".to_owned(), Value::String(blob_id.0));
            let outcome = create_batch(net, ty, vec![(format!("c{local}"), Value::Object(obj))])
                .map_err(Error::from)?;
            match outcome.created.first().and_then(|(_, v)| jid(v)) {
                Some(id) => {
                    counts.created += 1;
                    target_by_key.insert(key, id.clone());
                    id
                }
                None => {
                    for (cid, err) in &outcome.not_created {
                        logger.warn(&format!("SieveScript {cid} not created: {err}"));
                    }
                    counts.failed += 1;
                    continue;
                }
            }
        };
        if *is_active {
            active_target = Some(target_id);
        }
    }

    if active_target.is_none() && locals.iter().all(|(_, _, a, _)| !*a) {
        deactivate = true;
    }

    if !net.dry_run {
        let mut req = Request::new();
        let args = if let Some(id) = &active_target {
            json!({ "accountId": net.account, "onSuccessActivateScript": id })
        } else if deactivate {
            json!({ "accountId": net.account, "onSuccessDeactivateScript": true })
        } else {
            json!({ "accountId": net.account })
        };
        req.call("SieveScript/set", args, "a");
        if let Err(e) = req.send(&net.client, &net.api) {
            logger.warn(&format!("SieveScript activation failed: {e}"));
        }
    }

    let local_keys: std::collections::HashSet<[u8; 32]> = locals
        .iter()
        .filter_map(|(_, _, _, b)| {
            db::blobs::blob_bytes(&ctx.conn, *b)
                .ok()
                .flatten()
                .map(|by| blake3_bytes(&by))
        })
        .collect();
    let mut prune_candidates: Vec<String> = target_by_key
        .iter()
        .filter(|(k, _)| !local_keys.contains(*k))
        .map(|(_, id)| id.clone())
        .collect();
    prune_candidates.sort();

    Ok(Plan {
        prune_candidates,
        active_sieve_target: active_target,
    })
}
