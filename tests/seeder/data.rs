/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use calcard::icalendar::ICalendar;
use calcard::vcard::VCard;
use flate2::read::GzDecoder;
use mail_parser::mailbox::mbox::MessageIterator;
use serde_json::Value;

use super::error::{SeedError, SeedResult};

fn resources_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")
}

pub struct MboxMessage {
    pub raw: Vec<u8>,
    pub received_at: i64,
}

pub fn load_mbox(limit: usize) -> SeedResult<Vec<MboxMessage>> {
    let path = resources_dir().join("mailbox.gz");
    let file = std::fs::File::open(&path)?;
    let mut decoder = GzDecoder::new(file);
    let mut bytes = Vec::new();
    decoder.read_to_end(&mut bytes)?;
    let reader = BufReader::new(std::io::Cursor::new(bytes));
    let mut out = Vec::new();
    for item in MessageIterator::new(reader) {
        let message = item.map_err(|e| SeedError::Resource(format!("mbox parse: {e}")))?;
        let received_at = message.internal_date() as i64;
        out.push(MboxMessage {
            raw: message.unwrap_contents(),
            received_at,
        });
        if out.len() >= limit {
            break;
        }
    }
    if out.is_empty() {
        return Err(SeedError::Resource("mbox yielded no messages".to_owned()));
    }
    Ok(out)
}

fn read_dir_sorted(sub: &str, ext: &str) -> SeedResult<Vec<PathBuf>> {
    let dir = resources_dir().join(sub);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some(ext))
        .collect();
    paths.sort();
    Ok(paths)
}

pub fn load_contacts() -> SeedResult<Vec<Value>> {
    let mut cards = Vec::new();
    for path in read_dir_sorted("vcards", "vcf")? {
        let text = std::fs::read_to_string(&path)?;
        let vcard = VCard::parse(&text)
            .map_err(|_| SeedError::Resource(format!("vcard parse failed: {path:?}")))?;
        let js = vcard.into_jscontact::<String, String>();
        let value: Value = serde_json::from_str(&js.to_string_pretty())?;
        if has_nonempty_uid(&value) {
            cards.push(value);
        }
    }
    Ok(cards)
}

pub fn load_events() -> SeedResult<Vec<Value>> {
    let mut events = Vec::new();
    for path in read_dir_sorted("icals", "ics")? {
        let text = std::fs::read_to_string(&path)?;
        let ical = ICalendar::parse(&text)
            .map_err(|_| SeedError::Resource(format!("ical parse failed: {path:?}")))?;
        let js = ical.into_jscalendar::<String, String>();
        let value: Value = serde_json::from_str(&js.to_string_pretty())?;
        collect_events(&value, &mut events);
    }
    Ok(events)
}

fn collect_events(value: &Value, out: &mut Vec<Value>) {
    let at_type = value.get("@type").and_then(Value::as_str);
    match at_type {
        Some("Event") if has_nonempty_uid(value) => {
            out.push(value.clone());
        }
        Some("Group") => {
            if let Some(entries) = value.get("entries").and_then(Value::as_array) {
                for entry in entries {
                    collect_events(entry, out);
                }
            }
        }
        _ => {}
    }
}

fn has_nonempty_uid(value: &Value) -> bool {
    value
        .get("uid")
        .and_then(Value::as_str)
        .map(|u| !u.trim().is_empty())
        .unwrap_or(false)
}
