/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

pub fn encode_argument(s: &str, out: &mut String) {
    let bytes = s.as_bytes();
    if can_be_quoted(bytes) {
        out.push('"');
        for ch in s.chars() {
            if ch == '"' || ch == '\\' {
                out.push('\\');
            }
            out.push(ch);
        }
        out.push('"');
    } else {
        out.push('{');
        out.push_str(&bytes.len().to_string());
        out.push_str("+}\r\n");
        out.push_str(s);
    }
}

pub fn can_be_quoted(bytes: &[u8]) -> bool {
    if bytes.len() > 1024 {
        return false;
    }
    bytes
        .iter()
        .all(|&b| b != b'\r' && b != b'\n' && b != 0 && (b == b'\t' || (0x20..=0x7e).contains(&b)))
}

pub fn contains_literal(bytes: &[u8]) -> bool {
    bytes.contains(&b'{')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(s: &str) -> String {
        let mut out = String::new();
        encode_argument(s, &mut out);
        out
    }

    #[test]
    fn ascii_round_trips_as_quoted_string() {
        assert_eq!(encoded("vacation"), "\"vacation\"");
        assert_eq!(encoded(""), "\"\"");
    }

    #[test]
    fn quotes_and_backslashes_are_escaped() {
        assert_eq!(encoded("a\"b"), "\"a\\\"b\"");
        assert_eq!(encoded("a\\b"), "\"a\\\\b\"");
    }

    #[test]
    fn non_ascii_promotes_to_literal_with_byte_exact_body() {
        let name = "fancy\u{00e7}y";
        let out = encoded(name);
        let in_bytes = name.as_bytes();
        let header = format!("{{{}+}}\r\n", in_bytes.len());
        assert!(out.starts_with(&header), "got {out:?}");
        let body = &out.as_bytes()[header.len()..];
        assert_eq!(body, in_bytes, "literal body must be byte-exact UTF-8");
        assert_eq!(body.len(), in_bytes.len());
    }

    #[test]
    fn literal_length_matches_payload_byte_count_for_multibyte_runs() {
        for s in [
            "\u{00e7}",
            "na\u{00ef}ve",
            "\u{4e2d}\u{6587}",
            "mix-\u{00e9}\u{00e0}\u{00f6}",
        ] {
            let out = encoded(s);
            let wire = out.as_bytes();
            let header = format!("{{{}+}}\r\n", s.len());
            assert!(
                wire.starts_with(header.as_bytes()),
                "expected header {header:?} for {s:?}, got {out:?}"
            );
            let body = &wire[header.len()..];
            assert_eq!(body, s.as_bytes(), "literal body must equal input bytes");
        }
    }

    #[test]
    fn crlf_in_name_promotes_to_literal() {
        let out = encoded("with\nline");
        assert!(out.starts_with('{'));
        assert!(out.contains("+}\r\n"));
        let header = format!("{{{}+}}\r\n", "with\nline".len());
        assert!(out.starts_with(&header));
    }

    #[test]
    fn tab_is_allowed_in_quoted_string() {
        assert!(can_be_quoted(b"abc\tdef"));
    }

    #[test]
    fn nul_byte_forces_literal() {
        assert!(!can_be_quoted(b"a\0b"));
    }

    #[test]
    fn names_over_quoted_threshold_use_literal() {
        let long: Vec<u8> = (0..2048).map(|_| b'x').collect();
        assert!(!can_be_quoted(&long));
    }

    #[test]
    fn contains_literal_detects_open_brace() {
        assert!(contains_literal(b"GETSCRIPT {3+}\r\nabc\r\n"));
        assert!(!contains_literal(b"GETSCRIPT \"abc\"\r\n"));
    }
}
