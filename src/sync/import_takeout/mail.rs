/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::db;
use crate::db::takeout_ids;
use crate::error::Error;
use crate::logging::{LEVEL_PROGRESS, Logger};
use crate::sync::TypeCounts;
use crate::sync::emailmeta::email_meta_from_blob;
use crate::sync::keys::index_to_json;

use super::labels::{self, MappingOptions};
use super::mbox::{Message, MessageIterator};

pub struct InsertContext<'a> {
    pub source_id: i64,
    pub fallback_mailbox: &'a str,
    pub options: MappingOptions,
    pub mailbox_cache: &'a mut HashMap<String, i64>,
}

pub fn process_file(
    conn: &mut Connection,
    file_path: &Path,
    ctx: InsertContext<'_>,
    mailbox_counts: &mut TypeCounts,
    email_counts: &mut TypeCounts,
    logger: Logger,
) -> Result<(), Error> {
    let file =
        File::open(file_path).map_err(|e| Error::Partial(format!("open {file_path:?}: {e}")))?;
    let reader = BufReader::new(file);
    let mut iter = MessageIterator::new(reader);

    let InsertContext {
        source_id,
        fallback_mailbox,
        options,
        mailbox_cache,
    } = ctx;
    let tx = conn.transaction()?;
    let mut commit_pending: u64 = 0;

    loop {
        match iter.next() {
            None => break,
            Some(Err(e)) => {
                logger.warn(&format!("{file_path:?}: mbox io: {e}"));
                email_counts.failed += 1;
                break;
            }
            Some(Ok(msg)) => {
                match process_message(
                    &tx,
                    source_id,
                    fallback_mailbox,
                    options,
                    &msg,
                    mailbox_cache,
                    mailbox_counts,
                    email_counts,
                    logger,
                ) {
                    Ok(()) => {
                        commit_pending += 1;
                        if commit_pending.is_multiple_of(PROGRESS_TICK)
                            && logger.enabled(LEVEL_PROGRESS)
                        {
                            eprintln!("{file_path:?}: processed {commit_pending} messages");
                        }
                    }
                    Err(e) => {
                        logger.warn(&format!("{file_path:?}: message: {e}"));
                        email_counts.failed += 1;
                    }
                }
            }
        }
    }
    tx.commit()?;
    Ok(())
}

const PROGRESS_TICK: u64 = 256;

#[allow(clippy::too_many_arguments)]
fn process_message(
    tx: &Transaction<'_>,
    source_id: i64,
    fallback_mailbox: &str,
    options: MappingOptions,
    msg: &Message,
    cache: &mut HashMap<String, i64>,
    mailbox_counts: &mut TypeCounts,
    email_counts: &mut TypeCounts,
    logger: Logger,
) -> Result<(), Error> {
    let bytes = msg.contents();
    let msg_hash = blake3::hash(bytes).to_hex().to_string();

    let raw_header = extract_x_gmail_labels(bytes);
    let had_labels_header = raw_header.is_some();
    let tokens = match &raw_header {
        Some(s) => labels::parse_header(s),
        None => Vec::new(),
    };
    let mut classification = labels::classify(&tokens);
    if classification.mailboxes.is_empty() {
        let fallback = fallback_mailbox.trim();
        if fallback.is_empty() {
            classification.mailboxes.push("Imported".to_owned());
        } else {
            classification.mailboxes.push(fallback.to_owned());
        }
        if had_labels_header {
            logger.warn(&format!(
                "message {msg_hash}: X-Gmail-Labels present but empty after parse; \
                 placing in fallback mailbox {:?}",
                classification.mailboxes[0]
            ));
        } else {
            logger.warn(&format!(
                "message {msg_hash}: no X-Gmail-Labels header; placing in fallback mailbox {:?}",
                classification.mailboxes[0]
            ));
        }
    }
    if classification.opened_won_over_unread {
        logger.warn(&format!(
            "message {msg_hash}: X-Gmail-Labels carried both 'Opened' and 'Unread'; \
             treating as $seen (Opened wins)"
        ));
    }

    let mut mailbox_ids: Vec<i64> = Vec::with_capacity(classification.mailboxes.len());
    for path in &classification.mailboxes {
        let id = ensure_mailbox(tx, source_id, path, options, cache, mailbox_counts)?;
        mailbox_ids.push(id);
    }
    mailbox_ids.sort_unstable();
    mailbox_ids.dedup();

    let keywords_json = json_string_array(&classification.keywords_sorted());
    let mailbox_ids_json = json_int_array(&mailbox_ids);

    if let Some(local_id) = takeout_ids::local_for(tx, source_id, takeout_ids::EMAIL, &msg_hash)? {
        let current: (String, String) = tx
            .query_row(
                "SELECT mailbox_ids, keywords FROM emails WHERE id = ?1",
                params![local_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .unwrap_or_default();
        if current.0 != mailbox_ids_json || current.1 != keywords_json {
            tx.execute(
                "UPDATE emails SET mailbox_ids = ?1, keywords = ?2 WHERE id = ?3",
                params![mailbox_ids_json, keywords_json, local_id],
            )?;
            email_counts.fetched += 1;
        } else {
            email_counts.skipped += 1;
        }
        return Ok(());
    }

    let blob_id = db::blobs::intern_blob(tx, bytes)?;
    let (index, date_rfc3339) = email_meta_from_blob(bytes);
    let message_match = index_to_json(&index);
    let received_at = pick_received_at(msg.internal_date(), date_rfc3339.as_deref());

    tx.execute(
        "INSERT INTO emails (blob_id, received_at, mailbox_ids, keywords, message_match)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            blob_id,
            received_at,
            mailbox_ids_json,
            keywords_json,
            message_match
        ],
    )?;
    let local_id = tx.last_insert_rowid();
    takeout_ids::insert(tx, source_id, takeout_ids::EMAIL, &msg_hash, local_id)?;
    email_counts.created += 1;
    email_counts.fetched += 1;
    Ok(())
}

fn ensure_mailbox(
    tx: &Transaction<'_>,
    source_id: i64,
    path: &str,
    options: MappingOptions,
    cache: &mut HashMap<String, i64>,
    counts: &mut TypeCounts,
) -> Result<i64, Error> {
    if let Some(&id) = cache.get(path) {
        return Ok(id);
    }
    if let Some(id) = takeout_ids::local_for(tx, source_id, takeout_ids::MAILBOX, path)? {
        cache.insert(path.to_owned(), id);
        return Ok(id);
    }
    let (parent_id, leaf) = match path.rsplit_once('/') {
        Some((parent, leaf)) => (
            Some(ensure_mailbox(
                tx, source_id, parent, options, cache, counts,
            )?),
            leaf,
        ),
        None => (None, path),
    };
    let role = labels::role_for_mailbox(path, options);
    tx.execute(
        "INSERT INTO mailboxes (name, parent_id, role, sort_order, is_subscribed)
         VALUES (?1, ?2, ?3, 0, 1)",
        params![leaf, parent_id, role],
    )?;
    let id = tx.last_insert_rowid();
    takeout_ids::insert(tx, source_id, takeout_ids::MAILBOX, path, id)?;
    cache.insert(path.to_owned(), id);
    counts.created += 1;
    counts.fetched += 1;
    Ok(id)
}

fn json_string_array(values: &[String]) -> String {
    Value::Array(values.iter().map(|s| Value::String(s.clone())).collect()).to_string()
}

fn json_int_array(values: &[i64]) -> String {
    Value::Array(values.iter().map(|&i| Value::from(i)).collect()).to_string()
}

fn pick_received_at(internal_date: u64, date_rfc3339: Option<&str>) -> String {
    if internal_date > 0 {
        return format_unix_rfc3339(internal_date);
    }
    if let Some(d) = date_rfc3339 {
        return d.to_owned();
    }
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn format_unix_rfc3339(secs: u64) -> String {
    let when = UNIX_EPOCH + Duration::from_secs(secs);
    OffsetDateTime::from(when)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

fn extract_x_gmail_labels(bytes: &[u8]) -> Option<String> {
    let mut values: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut i = 0;
    while i < bytes.len() {
        let nl = bytes[i..].iter().position(|&b| b == b'\n');
        let (line, advance) = match nl {
            Some(p) => (&bytes[i..i + p], p + 1),
            None => (&bytes[i..], bytes.len() - i),
        };
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            break;
        }
        let is_continuation = line
            .first()
            .map(|b| *b == b' ' || *b == b'\t')
            .unwrap_or(false);
        if is_continuation {
            if let Some(buf) = current.as_mut() {
                let extra = std::str::from_utf8(line).unwrap_or("").trim_start();
                if !extra.is_empty() {
                    buf.push(' ');
                    buf.push_str(extra);
                }
            }
        } else {
            if let Some(buf) = current.take() {
                values.push(buf);
            }
            if let Some(colon) = line.iter().position(|&b| b == b':') {
                let name = &line[..colon];
                if name.eq_ignore_ascii_case(b"X-Gmail-Labels") {
                    let value = &line[colon + 1..];
                    let value = std::str::from_utf8(value).unwrap_or("").trim();
                    current = Some(value.to_owned());
                }
            }
        }
        i += advance;
    }
    if let Some(buf) = current.take() {
        values.push(buf);
    }
    if values.is_empty() {
        None
    } else {
        Some(values.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_labels_from_simple_message() {
        let bytes = b"From: a@b\r\n\
            X-Gmail-Labels: Inbox,Opened\r\n\
            Subject: hi\r\n\
            \r\n\
            body\r\n";
        assert_eq!(
            extract_x_gmail_labels(bytes),
            Some("Inbox,Opened".to_owned())
        );
    }

    #[test]
    fn extract_labels_handles_lf_only_line_endings() {
        let bytes = b"From: a@b\nX-Gmail-Labels: Inbox\n\nbody\n";
        assert_eq!(extract_x_gmail_labels(bytes), Some("Inbox".to_owned()));
    }

    #[test]
    fn extract_labels_handles_folded_continuation() {
        let bytes = b"From: a@b\r\n\
            X-Gmail-Labels: Inbox,\r\n\
            \tImportant,Github\r\n\
            Subject: hi\r\n\
            \r\n\
            body\r\n";
        assert_eq!(
            extract_x_gmail_labels(bytes),
            Some("Inbox, Important,Github".to_owned())
        );
    }

    #[test]
    fn extract_labels_returns_none_when_header_absent() {
        let bytes = b"From: a@b\r\nSubject: hi\r\n\r\nbody\r\n";
        assert_eq!(extract_x_gmail_labels(bytes), None);
    }

    #[test]
    fn extract_labels_case_insensitive_name_match() {
        let bytes = b"x-gmail-labels: Sent\r\n\r\n";
        assert_eq!(extract_x_gmail_labels(bytes), Some("Sent".to_owned()));
    }

    #[test]
    fn extract_labels_concatenates_multiple_header_occurrences() {
        let bytes = b"X-Gmail-Labels: Inbox\r\nX-Gmail-Labels: Github\r\n\r\nbody";
        assert_eq!(
            extract_x_gmail_labels(bytes),
            Some("Inbox,Github".to_owned())
        );
    }

    #[test]
    fn extract_labels_stops_at_blank_line() {
        let bytes = b"X-Gmail-Labels: Inbox\r\n\r\nX-Gmail-Labels: NOT-A-HEADER\r\n";
        assert_eq!(extract_x_gmail_labels(bytes), Some("Inbox".to_owned()));
    }

    #[test]
    fn json_int_array_emits_compact_form() {
        assert_eq!(json_int_array(&[1, 2, 3]), "[1,2,3]");
        assert_eq!(json_int_array(&[]), "[]");
    }

    #[test]
    fn json_string_array_quotes_each_value() {
        assert_eq!(
            json_string_array(&["$seen".to_owned(), "$flagged".to_owned()]),
            "[\"$seen\",\"$flagged\"]"
        );
    }

    #[test]
    fn pick_received_at_prefers_envelope_internal_date() {
        let got = pick_received_at(1763065233, Some("2025-05-12T10:00:00+02:00"));
        assert_eq!(got, "2025-11-13T20:20:33Z");
    }

    #[test]
    fn pick_received_at_falls_back_to_date_header() {
        let got = pick_received_at(0, Some("2025-05-12T10:00:00+02:00"));
        assert_eq!(got, "2025-05-12T10:00:00+02:00");
    }

    #[test]
    fn pick_received_at_last_resort_is_now_in_rfc3339() {
        let got = pick_received_at(0, None);
        assert!(got.ends_with("Z") || got.contains("+") || got.contains("-"));
        assert!(got.contains("T"));
    }
}
