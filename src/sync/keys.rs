/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde_json::{Value, json};

pub fn normalize_message_id(raw: &str) -> String {
    let t = raw.trim();
    let t = t.strip_prefix('<').unwrap_or(t);
    let t = t.strip_suffix('>').unwrap_or(t);
    t.trim().to_lowercase()
}

pub fn fold_name(name: &str) -> String {
    name.trim().to_lowercase()
}

pub fn blake3_fields(fields: &[&str]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            hasher.update(b"\x1f");
        }
        hasher.update(f.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

pub fn blake3_bytes(bytes: &[u8]) -> [u8; 32] {
    *blake3::hash(bytes).as_bytes()
}

pub fn identity_key(name: &str, email: &str) -> [u8; 32] {
    blake3_fields(&[name, &email.to_lowercase()])
}

pub fn participant_identity_key(calendar_address: &str, name: &str) -> [u8; 32] {
    blake3_fields(&[&calendar_address.to_lowercase(), name])
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum EmailKey {
    MessageId(String),
    Fallback([u8; 32]),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmailIndex {

    pub mids: Vec<String>,

    pub fb: [u8; 32],
}

fn join_addrs(addrs: &[String]) -> String {
    addrs
        .iter()
        .map(|a| a.to_lowercase())
        .collect::<Vec<_>>()
        .join(",")
}

pub fn email_index(
    message_ids: &[String],
    from: &[String],
    subject: &str,
    sent_at: &str,
    to: &[String],
) -> EmailIndex {
    let mut mids: Vec<String> = message_ids
        .iter()
        .map(|m| normalize_message_id(m))
        .filter(|s| !s.is_empty())
        .collect();
    mids.sort();
    mids.dedup();
    let fb = blake3_fields(&[&join_addrs(from), subject, sent_at, &join_addrs(to)]);
    EmailIndex { mids, fb }
}

pub fn index_to_json(idx: &EmailIndex) -> String {
    json!({ "m": idx.mids, "f": hex(&idx.fb) }).to_string()
}

pub fn index_from_json(text: &str) -> EmailIndex {
    let v: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let mids = v
        .get("m")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let fb = v
        .get("f")
        .and_then(Value::as_str)
        .and_then(unhex)
        .unwrap_or([0u8; 32]);
    EmailIndex { mids, fb }
}

fn hex(bytes: &[u8; 32]) -> String {
    let mut s = String::with_capacity(64);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

pub fn email_key(idx: &EmailIndex) -> EmailKey {
    if idx.mids.is_empty() {
        EmailKey::Fallback(idx.fb)
    } else {
        EmailKey::MessageId(idx.mids.join("\x1f"))
    }
}

pub fn email_keys(indices: &[EmailIndex]) -> Vec<EmailKey> {
    indices.iter().map(email_key).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_id_normalization_strips_brackets_and_lowercases() {
        assert_eq!(normalize_message_id("  <ABC@Host.COM> "), "abc@host.com");
        assert_eq!(normalize_message_id("plain@x"), "plain@x");
    }

    #[test]
    fn name_fold_is_case_insensitive() {
        assert_eq!(fold_name(" Sales "), fold_name("sales"));
        assert_ne!(fold_name("Sales"), fold_name("sale"));
    }

    #[test]
    fn identity_collapse_ignores_signature() {
        assert_eq!(
            identity_key("Alice", "A@x.test"),
            identity_key("Alice", "a@x.test")
        );
    }

    #[test]
    fn participant_identity_key_lowercases_address_only() {
        let a = participant_identity_key("MailTo:User@Host", "Bob");
        let b = participant_identity_key("mailto:user@host", "Bob");
        assert_eq!(a, b);
        assert_ne!(a, participant_identity_key("mailto:user@host", "bob"));
    }

    fn idx(mids: &[&str], from: &[&str], subj: &str, sent: &str, to: &[&str]) -> EmailIndex {
        let m: Vec<String> = mids.iter().map(|s| (*s).to_owned()).collect();
        let f: Vec<String> = from.iter().map(|s| (*s).to_owned()).collect();
        let t: Vec<String> = to.iter().map(|s| (*s).to_owned()).collect();
        email_index(&m, &f, subj, sent, &t)
    }

    #[test]
    fn message_id_array_is_normalized_sorted_and_order_independent() {
        let a = idx(&["<B@h>", "<a@H>"], &[], "", "", &[]);
        let b = idx(&["<a@h>", "<b@h>"], &[], "", "", &[]);
        assert_eq!(a.mids, vec!["a@h".to_owned(), "b@h".to_owned()]);
        assert_eq!(a.mids, b.mids);
    }

    #[test]
    fn unique_single_message_id_uses_message_id_key() {
        let keys = email_keys(&[
            idx(
                &["<m1@h>"],
                &["a@x"],
                "Hi",
                "2020-01-01T00:00:00Z",
                &["b@y"],
            ),
            idx(
                &["<m2@h>"],
                &["a@x"],
                "Yo",
                "2020-01-02T00:00:00Z",
                &["b@y"],
            ),
        ]);
        assert_eq!(keys[0], EmailKey::MessageId("m1@h".to_owned()));
        assert_eq!(keys[1], EmailKey::MessageId("m2@h".to_owned()));
    }

    #[test]
    fn shared_message_id_matches_regardless_of_other_fields() {
        let keys = email_keys(&[
            idx(
                &["<dup@h>"],
                &["a@x"],
                "S",
                "2020-01-01T00:00:00Z",
                &["b@y"],
            ),
            idx(
                &["<dup@h>"],
                &["c@x"],
                "T",
                "2020-01-02T00:00:00Z",
                &["d@y"],
            ),
        ]);
        assert_eq!(keys[0], EmailKey::MessageId("dup@h".to_owned()));
        assert_eq!(keys[0], keys[1], "same Message-ID is the same message");
    }

    #[test]
    fn only_absent_message_id_falls_back_multi_is_still_message_id() {
        let keys = email_keys(&[
            idx(&[], &["a@x"], "S", "2020-01-01T00:00:00Z", &["b@y"]),
            idx(
                &["<a@h>", "<b@h>"],
                &["a@x"],
                "S",
                "2020-01-01T00:00:00Z",
                &["b@y"],
            ),
        ]);
        assert!(matches!(keys[0], EmailKey::Fallback(_)));
        assert_eq!(keys[1], EmailKey::MessageId("a@h\u{1f}b@h".to_owned()));
    }

    #[test]
    fn fallback_is_case_insensitive_on_addresses_and_offset_preserving_on_sent_at() {
        let a = idx(&[], &["A@X"], "Subj", "2020-01-01T10:00:00+02:00", &["B@Y"]);
        let b = idx(&[], &["a@x"], "Subj", "2020-01-01T10:00:00+02:00", &["b@y"]);
        let c = idx(&[], &["a@x"], "Subj", "2020-01-01T08:00:00Z", &["b@y"]);
        assert_eq!(a.fb, b.fb);
        assert_ne!(a.fb, c.fb);
    }

    #[test]
    fn index_json_roundtrips() {
        let i = idx(&["<m@h>"], &["a@x"], "S", "2020-01-01T00:00:00Z", &["b@y"]);
        let j = index_to_json(&i);
        let back = index_from_json(&j);
        assert_eq!(i, back);
        assert_eq!(email_keys(&[i]), email_keys(&[back]));
    }

    #[test]
    fn malformed_stored_json_degrades_to_empty_fallback() {
        let back = index_from_json("{}");
        assert!(back.mids.is_empty());
        assert_eq!(back.fb, [0u8; 32]);
    }
}
