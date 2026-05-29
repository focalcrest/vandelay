/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::io::{self, BufRead};

use super::error::{NoError, SieveError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    Atom(String),
    QStr(String),
    Literal(Vec<u8>),
    LParen,
    RParen,
}

impl Token {
    pub fn as_string_bytes(&self) -> Option<Vec<u8>> {
        match self {
            Token::QStr(s) => Some(s.as_bytes().to_vec()),
            Token::Literal(b) => Some(b.clone()),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<String> {
        match self {
            Token::QStr(s) => Some(s.clone()),
            Token::Literal(b) => Some(String::from_utf8_lossy(b).into_owned()),
            Token::Atom(s) => Some(s.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Ok,
    No,
    Bye,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub status: Status,
    pub code: Option<String>,
    pub code_args: Option<String>,
    pub text: String,
}

impl Default for StatusLine {
    fn default() -> Self {
        StatusLine {
            status: Status::Ok,
            code: None,
            code_args: None,
            text: String::new(),
        }
    }
}

impl StatusLine {
    pub fn into_no_error(self) -> NoError {
        NoError::new(self.text, self.code)
    }
}

#[derive(Debug, Default)]
pub struct ResponseBlock {
    pub data: Vec<Vec<Token>>,
    pub status: StatusLine,
}

pub fn read_response<R: BufRead>(reader: &mut R) -> Result<ResponseBlock, SieveError> {
    let mut data: Vec<Vec<Token>> = Vec::new();
    loop {
        let line = read_logical_line(reader)?;
        if line.is_empty() {
            continue;
        }
        if let Some(status) = try_parse_status(&line) {
            return Ok(ResponseBlock { data, status });
        }
        data.push(line);
    }
}

pub fn try_parse_status(tokens: &[Token]) -> Option<StatusLine> {
    let first = tokens.first()?;
    let status = match first {
        Token::Atom(s) => match s.as_str() {
            "OK" | "ok" | "Ok" => Status::Ok,
            "NO" | "no" | "No" => Status::No,
            "BYE" | "bye" | "Bye" => Status::Bye,
            _ => return None,
        },
        _ => return None,
    };
    let mut idx = 1usize;
    let mut code: Option<String> = None;
    let mut code_args: Option<String> = None;
    if matches!(tokens.get(idx), Some(Token::LParen)) {
        idx += 1;
        let mut depth: i32 = 1;
        let mut inside: Vec<Token> = Vec::new();
        while idx < tokens.len() && depth > 0 {
            match &tokens[idx] {
                Token::LParen => {
                    depth += 1;
                    inside.push(tokens[idx].clone());
                }
                Token::RParen => {
                    depth -= 1;
                    if depth > 0 {
                        inside.push(tokens[idx].clone());
                    }
                }
                t => inside.push(t.clone()),
            }
            idx += 1;
        }
        if !inside.is_empty() {
            if let Some(name) = inside[0].as_string() {
                code = Some(name);
            }
            if inside.len() > 1 {
                let mut joined = String::new();
                for (i, t) in inside.iter().enumerate().skip(1) {
                    if let Some(s) = t.as_string() {
                        if i > 1 {
                            joined.push(' ');
                        }
                        joined.push_str(&s);
                    }
                }
                if !joined.is_empty() {
                    code_args = Some(joined);
                }
            }
        }
    }
    let text = match tokens.get(idx) {
        Some(Token::QStr(s)) => s.clone(),
        Some(Token::Literal(b)) => String::from_utf8_lossy(b).into_owned(),
        Some(Token::Atom(s)) => s.clone(),
        _ => String::new(),
    };
    Some(StatusLine {
        status,
        code,
        code_args,
        text,
    })
}

fn read_logical_line<R: BufRead>(reader: &mut R) -> Result<Vec<Token>, SieveError> {
    let mut tokens: Vec<Token> = Vec::new();
    let mut physical = Vec::with_capacity(256);
    read_physical_line(reader, &mut physical)?;
    let mut pos = 0usize;
    loop {
        while pos < physical.len() && matches!(physical[pos], b' ' | b'\t') {
            pos += 1;
        }
        if pos >= physical.len() {
            return Ok(tokens);
        }
        match physical[pos] {
            b'"' => {
                let (s, new_pos) = parse_quoted_string(&physical, pos + 1)?;
                tokens.push(Token::QStr(s));
                pos = new_pos;
            }
            b'{' => {
                let (n, plus, new_pos) = parse_literal_marker(&physical, pos + 1)?;
                let _ = plus;
                let trailing = &physical[new_pos..];
                if trailing.iter().any(|b| !matches!(b, b' ' | b'\t')) {
                    return Err(SieveError::Parse(format!(
                        "literal marker {{{n}}} not at end of line: trailing {:?}",
                        String::from_utf8_lossy(trailing)
                    )));
                }
                let mut data = vec![0u8; n];
                reader.read_exact(&mut data)?;
                tokens.push(Token::Literal(data));
                physical.clear();
                read_physical_line(reader, &mut physical)?;
                pos = 0;
            }
            b'(' => {
                tokens.push(Token::LParen);
                pos += 1;
            }
            b')' => {
                tokens.push(Token::RParen);
                pos += 1;
            }
            _ => {
                let start = pos;
                while pos < physical.len()
                    && !matches!(physical[pos], b' ' | b'\t' | b'"' | b'{' | b'(' | b')')
                {
                    pos += 1;
                }
                let atom = String::from_utf8_lossy(&physical[start..pos]).into_owned();
                tokens.push(Token::Atom(atom));
            }
        }
    }
}

fn read_physical_line<R: BufRead>(reader: &mut R, buf: &mut Vec<u8>) -> Result<(), SieveError> {
    buf.clear();
    let n = reader.read_until(b'\n', buf)?;
    if n == 0 {
        return Err(SieveError::Io(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "managesieve closed stream",
        )));
    }
    while matches!(buf.last(), Some(b'\n') | Some(b'\r')) {
        buf.pop();
    }
    Ok(())
}

fn parse_quoted_string(buf: &[u8], start: usize) -> Result<(String, usize), SieveError> {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = start;
    while i < buf.len() {
        match buf[i] {
            b'"' => {
                let s = String::from_utf8_lossy(&bytes).into_owned();
                return Ok((s, i + 1));
            }
            b'\\' if i + 1 < buf.len() => {
                bytes.push(buf[i + 1]);
                i += 2;
            }
            _ => {
                bytes.push(buf[i]);
                i += 1;
            }
        }
    }
    Err(SieveError::Parse("unterminated quoted-string".into()))
}

fn parse_literal_marker(buf: &[u8], start: usize) -> Result<(usize, bool, usize), SieveError> {
    let mut i = start;
    let mut n: usize = 0;
    let digits_start = i;
    while i < buf.len() && buf[i].is_ascii_digit() {
        n = n
            .checked_mul(10)
            .and_then(|v| v.checked_add((buf[i] - b'0') as usize))
            .ok_or_else(|| SieveError::Parse("literal size overflow".into()))?;
        i += 1;
    }
    if i == digits_start {
        return Err(SieveError::Parse("literal marker missing digits".into()));
    }
    let plus = i < buf.len() && buf[i] == b'+';
    if plus {
        i += 1;
    }
    if i >= buf.len() || buf[i] != b'}' {
        return Err(SieveError::Parse(
            "literal marker missing closing brace".into(),
        ));
    }
    Ok((n, plus, i + 1))
}

#[derive(Debug, Default, Clone)]
pub struct ListedScript {
    pub name: String,
    pub active: bool,
}

pub fn parse_listscripts(data: &[Vec<Token>]) -> Result<Vec<ListedScript>, SieveError> {
    let mut out = Vec::with_capacity(data.len());
    for line in data {
        let mut iter = line.iter();
        let first = match iter.next() {
            Some(t) => t,
            None => continue,
        };
        let name = first
            .as_string()
            .ok_or_else(|| SieveError::Parse(format!("LISTSCRIPTS line missing name: {line:?}")))?;
        let active = iter.any(|t| matches!(t, Token::Atom(a) if a.eq_ignore_ascii_case("ACTIVE")));
        out.push(ListedScript { name, active });
    }
    Ok(out)
}

pub fn parse_getscript(data: &[Vec<Token>]) -> Result<Vec<u8>, SieveError> {
    let mut buf: Vec<u8> = Vec::new();
    let mut found = false;
    for line in data {
        for token in line {
            match token {
                Token::Literal(b) => {
                    if found {
                        return Err(SieveError::Parse(
                            "GETSCRIPT returned more than one body token".into(),
                        ));
                    }
                    buf = b.clone();
                    found = true;
                }
                Token::QStr(s) => {
                    if found {
                        return Err(SieveError::Parse(
                            "GETSCRIPT returned more than one body token".into(),
                        ));
                    }
                    buf = s.as_bytes().to_vec();
                    found = true;
                }
                _ => {}
            }
        }
    }
    if !found {
        return Ok(Vec::new());
    }
    Ok(buf)
}

pub fn parse_capabilities(data: &[Vec<Token>]) -> Capabilities {
    let mut caps = Capabilities::default();
    for line in data {
        if line.is_empty() {
            continue;
        }
        let name = match line[0].as_string() {
            Some(s) => s,
            None => continue,
        };
        let value = if line.len() > 1 {
            line[1..]
                .iter()
                .filter_map(|t| t.as_string())
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            String::new()
        };
        match name.to_ascii_uppercase().as_str() {
            "IMPLEMENTATION" => caps.implementation = Some(value),
            "VERSION" => caps.version = Some(value),
            "SASL" => {
                caps.sasl = value
                    .split_ascii_whitespace()
                    .map(|s| s.to_ascii_uppercase())
                    .collect()
            }
            "SIEVE" => caps.sieve = Some(value),
            "STARTTLS" => caps.starttls = true,
            "MAXREDIRECTS" => caps.maxredirects = value.parse().ok(),
            "NOTIFY" => caps.notify = Some(value),
            "LANGUAGE" => caps.language = Some(value),
            "OWNER" => caps.owner = Some(value),
            other => {
                caps.raw.push((other.to_ascii_uppercase(), value));
            }
        }
    }
    caps
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub implementation: Option<String>,
    pub version: Option<String>,
    pub sasl: Vec<String>,
    pub sieve: Option<String>,
    pub starttls: bool,
    pub maxredirects: Option<u32>,
    pub notify: Option<String>,
    pub language: Option<String>,
    pub owner: Option<String>,
    pub raw: Vec<(String, String)>,
}

impl Capabilities {
    pub fn has_sasl(&self, mech: &str) -> bool {
        self.sasl.iter().any(|m| m.eq_ignore_ascii_case(mech))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn cursor(s: &[u8]) -> Cursor<Vec<u8>> {
        Cursor::new(s.to_vec())
    }

    #[test]
    fn parses_greeting_with_quoted_pairs_then_ok() {
        let input =
            b"\"IMPLEMENTATION\" \"Test Server\"\r\n\"VERSION\" \"1.0\"\r\n\"STARTTLS\"\r\n\"SASL\" \"PLAIN LOGIN\"\r\nOK \"hello\"\r\n";
        let mut c = cursor(input);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.status.status, Status::Ok);
        assert_eq!(r.status.text, "hello");
        let caps = parse_capabilities(&r.data);
        assert_eq!(caps.implementation, Some("Test Server".to_owned()));
        assert_eq!(caps.version, Some("1.0".to_owned()));
        assert!(caps.starttls);
        assert!(caps.has_sasl("PLAIN"));
        assert!(caps.has_sasl("login"));
        assert!(!caps.has_sasl("XOAUTH2"));
    }

    #[test]
    fn parses_listscripts_with_active_marker() {
        let input = b"\"vacation\"\r\n\"out-of-office\" ACTIVE\r\n\"forward\"\r\nOK\r\n";
        let mut c = cursor(input);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.status.status, Status::Ok);
        let list = parse_listscripts(&r.data).unwrap();
        assert_eq!(list.len(), 3);
        assert!(!list[0].active);
        assert!(list[1].active);
        assert_eq!(list[1].name, "out-of-office");
        assert!(!list[2].active);
    }

    #[test]
    fn parses_listscripts_with_literal_name_then_active_on_same_logical_line() {
        let input = b"{12+}\r\nfun\xc3\xa7y-name SP ACTIVE\r\nOK\r\n";
        let _ = input;
        let body = b"\x67\xc3\xa9\x6e\xc3\xa9\x72\x69\x71\x75\x65";
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(format!("{{{}}}\r\n", body.len()).as_bytes());
        data.extend_from_slice(body);
        data.extend_from_slice(b" ACTIVE\r\n");
        data.extend_from_slice(b"OK\r\n");
        let mut c = cursor(&data);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.status.status, Status::Ok);
        let list = parse_listscripts(&r.data).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name.as_bytes(), body);
        assert!(list[0].active);
    }

    #[test]
    fn parses_getscript_literal_body() {
        let script = b"require [\"fileinto\"];\nif true { fileinto \"INBOX\"; }\n";
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(format!("{{{}}}\r\n", script.len()).as_bytes());
        data.extend_from_slice(script);
        data.extend_from_slice(b"\r\nOK\r\n");
        let mut c = cursor(&data);
        let r = read_response(&mut c).unwrap();
        let body = parse_getscript(&r.data).unwrap();
        assert_eq!(body, script);
    }

    #[test]
    fn parses_getscript_quoted_one_line_body() {
        let data = b"\"stop;\"\r\nOK\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        let body = parse_getscript(&r.data).unwrap();
        assert_eq!(body, b"stop;");
    }

    #[test]
    fn parses_no_with_code_and_text() {
        let data = b"NO (NONEXISTENT) \"no such script\"\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.status.status, Status::No);
        assert_eq!(r.status.code.as_deref(), Some("NONEXISTENT"));
        assert_eq!(r.status.text, "no such script");
    }

    #[test]
    fn parses_no_without_code_and_quoted_text() {
        let data = b"NO \"failed\"\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.status.status, Status::No);
        assert!(r.status.code.is_none());
        assert_eq!(r.status.text, "failed");
    }

    #[test]
    fn parses_bye_referral_with_code_args() {
        let data = b"BYE (REFERRAL \"sieve://other.example/\") \"go away\"\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.status.status, Status::Bye);
        assert_eq!(r.status.code.as_deref(), Some("REFERRAL"));
        assert_eq!(r.status.text, "go away");
        assert!(r.status.code_args.unwrap().contains("other.example"));
    }

    #[test]
    fn parses_bare_ok_response() {
        let data = b"OK\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.status.status, Status::Ok);
        assert!(r.status.text.is_empty());
        assert!(r.data.is_empty());
    }

    #[test]
    fn quoted_string_escapes_backslash_and_quote() {
        let data = b"\"a \\\"b\\\\\" \r\nOK\r\n";
        let _ = data;
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"\"a \\\"b\\\\\"\r\nOK\r\n");
        let mut c = cursor(&buf);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.data.len(), 1);
        match &r.data[0][0] {
            Token::QStr(s) => assert_eq!(s, "a \"b\\"),
            other => panic!("expected QStr, got {other:?}"),
        }
    }

    #[test]
    fn literal_size_overflow_is_parse_error() {
        let data = b"{99999999999999999999}\r\nbody\r\nOK\r\n";
        let mut c = cursor(data);
        let err = read_response(&mut c).unwrap_err();
        assert!(matches!(err, SieveError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn unterminated_quoted_string_is_parse_error() {
        let data = b"\"unterminated\r\n";
        let mut c = cursor(data);
        let err = read_response(&mut c).unwrap_err();
        assert!(matches!(err, SieveError::Parse(_)), "got {err:?}");
    }

    #[test]
    fn capability_atoms_recognised_without_quotes() {
        let data = b"IMPLEMENTATION \"X\"\r\nSTARTTLS\r\nSASL PLAIN\r\nOK\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        let caps = parse_capabilities(&r.data);
        assert_eq!(caps.implementation.as_deref(), Some("X"));
        assert!(caps.starttls);
        assert!(caps.has_sasl("PLAIN"));
    }

    #[test]
    fn capability_sasl_as_multiple_unquoted_atoms_captures_every_mechanism() {
        let data = b"IMPLEMENTATION \"X\"\r\nSASL PLAIN LOGIN OAUTHBEARER\r\nOK\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        let caps = parse_capabilities(&r.data);
        assert!(caps.has_sasl("PLAIN"), "PLAIN missing: {:?}", caps.sasl);
        assert!(caps.has_sasl("LOGIN"), "LOGIN missing: {:?}", caps.sasl);
        assert!(
            caps.has_sasl("OAUTHBEARER"),
            "OAUTHBEARER missing: {:?}",
            caps.sasl
        );
    }

    #[test]
    fn capability_sasl_as_single_quoted_string_captures_every_mechanism() {
        let data = b"\"SASL\" \"PLAIN LOGIN OAUTHBEARER\"\r\nOK\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        let caps = parse_capabilities(&r.data);
        assert!(caps.has_sasl("PLAIN"));
        assert!(caps.has_sasl("LOGIN"));
        assert!(caps.has_sasl("OAUTHBEARER"));
    }

    #[test]
    fn quoted_string_preserves_non_ascii_utf8_bytes() {
        let name_bytes: &[u8] = b"fancy\xc3\xa7y";
        let mut data: Vec<u8> = Vec::new();
        data.push(b'"');
        data.extend_from_slice(name_bytes);
        data.push(b'"');
        data.extend_from_slice(b"\r\nOK\r\n");
        let mut c = cursor(&data);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.data.len(), 1);
        match &r.data[0][0] {
            Token::QStr(s) => assert_eq!(s.as_bytes(), name_bytes),
            other => panic!("expected QStr, got {other:?}"),
        }
    }

    #[test]
    fn listscripts_quoted_name_with_non_ascii_is_byte_exact() {
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(b"\"fun\xc3\xa7y\"\r\nOK\r\n");
        let mut c = cursor(&data);
        let r = read_response(&mut c).unwrap();
        let list = parse_listscripts(&r.data).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name.as_bytes(), b"fun\xc3\xa7y");
    }

    #[test]
    fn parses_two_logical_lines_with_intervening_blank_skipped() {
        let data = b"\"a\"\r\n\r\n\"b\"\r\nOK\r\n";
        let mut c = cursor(data);
        let r = read_response(&mut c).unwrap();
        assert_eq!(r.data.len(), 2);
    }
}
