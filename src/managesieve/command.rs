/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use super::name::encode_argument;

pub fn capability() -> String {
    "CAPABILITY\r\n".to_owned()
}

pub fn starttls() -> String {
    "STARTTLS\r\n".to_owned()
}

pub fn noop() -> String {
    "NOOP\r\n".to_owned()
}

pub fn logout() -> String {
    "LOGOUT\r\n".to_owned()
}

pub fn listscripts() -> String {
    "LISTSCRIPTS\r\n".to_owned()
}

pub fn getscript(name: &str) -> String {
    let mut out = String::with_capacity(name.len() + 16);
    out.push_str("GETSCRIPT ");
    encode_argument(name, &mut out);
    out.push_str("\r\n");
    out
}

pub fn authenticate(mechanism: &str) -> String {
    let mut out = String::with_capacity(mechanism.len() + 20);
    out.push_str("AUTHENTICATE ");
    encode_argument(mechanism, &mut out);
    out.push_str("\r\n");
    out
}

pub fn authenticate_with_initial(mechanism: &str, initial_b64: &str) -> String {
    let mut out = String::with_capacity(mechanism.len() + initial_b64.len() + 32);
    out.push_str("AUTHENTICATE ");
    encode_argument(mechanism, &mut out);
    out.push(' ');
    encode_argument(initial_b64, &mut out);
    out.push_str("\r\n");
    out
}

pub fn continuation_payload(initial_b64: &str) -> String {
    let mut out = String::with_capacity(initial_b64.len() + 8);
    encode_argument(initial_b64, &mut out);
    out.push_str("\r\n");
    out
}

pub use super::name::contains_literal;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_is_uppercase_with_crlf() {
        assert_eq!(capability(), "CAPABILITY\r\n");
    }

    #[test]
    fn getscript_quoted_for_ascii_names() {
        assert_eq!(getscript("vacation"), "GETSCRIPT \"vacation\"\r\n");
    }

    #[test]
    fn getscript_escapes_quotes_and_backslashes() {
        let out = getscript("a\"b\\c");
        assert!(out.contains("\\\""));
        assert!(out.contains("\\\\"));
    }

    #[test]
    fn getscript_uses_literal_for_non_ascii() {
        let name = "fancy\u{00e7}y";
        let out = getscript(name);
        let nbytes = name.len();
        let expected = {
            let mut v: Vec<u8> = Vec::new();
            v.extend_from_slice(format!("GETSCRIPT {{{nbytes}+}}\r\n").as_bytes());
            v.extend_from_slice(name.as_bytes());
            v.extend_from_slice(b"\r\n");
            v
        };
        assert_eq!(
            out.as_bytes(),
            expected.as_slice(),
            "GETSCRIPT wire bytes for non-ascii name must be byte-exact"
        );
    }

    #[test]
    fn getscript_uses_literal_for_names_containing_crlf() {
        let name = "with\nlinebreak";
        let out = getscript(name);
        assert!(
            out.contains("{") && out.contains("+}\r\n"),
            "expected literal for newline-bearing name: {out:?}"
        );
    }

    #[test]
    fn authenticate_with_initial_emits_two_arguments() {
        let out = authenticate_with_initial("PLAIN", "AGZvbwBiYXI=");
        assert_eq!(out, "AUTHENTICATE \"PLAIN\" \"AGZvbwBiYXI=\"\r\n");
    }

    #[test]
    fn authenticate_without_initial_emits_one_argument() {
        let out = authenticate("LOGIN");
        assert_eq!(out, "AUTHENTICATE \"LOGIN\"\r\n");
    }

    #[test]
    fn continuation_payload_quotes_simple_base64() {
        assert_eq!(continuation_payload("Zm9v"), "\"Zm9v\"\r\n");
    }

    #[test]
    fn contains_literal_detects_open_brace() {
        assert!(contains_literal(b"GETSCRIPT {3+}\r\nabc\r\n"));
        assert!(!contains_literal(b"GETSCRIPT \"abc\"\r\n"));
    }
}
