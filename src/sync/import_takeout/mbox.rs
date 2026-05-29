/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use std::io::BufRead;

use time::{Date, Month, OffsetDateTime, Time, UtcOffset};

pub struct MessageIterator<T> {
    reader: T,
    message: Option<Message>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Message {
    internal_date: u64,
    envelope_from: String,
    contents: Vec<u8>,
}

impl<T> MessageIterator<T>
where
    T: BufRead,
{
    pub fn new(reader: T) -> MessageIterator<T> {
        MessageIterator {
            reader,
            message: None,
        }
    }
}

impl<T> Iterator for MessageIterator<T>
where
    T: BufRead,
{
    type Item = std::io::Result<Message>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut line = Vec::with_capacity(80);

        loop {
            match self.reader.read_until(b'\n', &mut line) {
                Ok(0) => return self.message.take().map(Ok),
                Ok(_) => {}
                Err(e) => return Some(Err(e)),
            }

            if line.starts_with(b"From ") {
                let finished = self.message.take().map(Ok);
                let header = std::str::from_utf8(&line).unwrap_or("");
                self.message = Some(Message::from_envelope(header));
                if finished.is_some() {
                    return finished;
                }
                line.clear();
                continue;
            }

            if let Some(message) = &mut self.message {
                if line.first() != Some(&b'>') {
                    message.contents.extend_from_slice(&line);
                    line.clear();
                    continue;
                }
                let non_gt = line.iter().position(|&c| c != b'>').unwrap_or(line.len());
                if line[non_gt..].starts_with(b"From ") {
                    message.contents.extend_from_slice(&line[1..]);
                } else {
                    message.contents.extend_from_slice(&line);
                }
            }
            line.clear();
        }
    }
}

impl Message {
    fn from_envelope(header: &str) -> Self {
        let trimmed = header.strip_prefix("From ").unwrap_or(header);
        let (envelope_from, date_str) = match trimmed.split_once(' ') {
            Some((from, rest)) => (from.trim().to_owned(), rest),
            None => (String::new(), ""),
        };
        let internal_date = parse_envelope_date(date_str).unwrap_or(0);
        Self {
            internal_date,
            envelope_from,
            contents: Vec::with_capacity(1024),
        }
    }

    pub fn internal_date(&self) -> u64 {
        self.internal_date
    }

    pub fn envelope_from(&self) -> &str {
        &self.envelope_from
    }

    pub fn contents(&self) -> &[u8] {
        &self.contents
    }

    pub fn unwrap_contents(self) -> Vec<u8> {
        self.contents
    }
}

fn parse_envelope_date(s: &str) -> Option<u64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    let (month_pos, day_pos, time_pos, offset_pos, year_pos) = match parts.len() {
        5 => (1, 2, 3, None, 4),
        6 => (1, 2, 3, Some(4), 5),
        _ => return None,
    };

    let month = parse_month(parts[month_pos])?;
    let day: u8 = parts[day_pos].parse().ok()?;
    let (hour, minute, second) = parse_hms(parts[time_pos])?;
    let year: i32 = parts[year_pos].parse().ok()?;
    let offset = match offset_pos {
        Some(p) => parse_offset(parts[p])?,
        None => UtcOffset::UTC,
    };

    let date = Date::from_calendar_date(year, month, day).ok()?;
    let time = Time::from_hms(hour, minute, second).ok()?;
    let ts = OffsetDateTime::new_in_offset(date, time, offset).unix_timestamp();
    u64::try_from(ts).ok()
}

fn parse_month(s: &str) -> Option<Month> {
    Some(match s {
        x if x.eq_ignore_ascii_case("jan") => Month::January,
        x if x.eq_ignore_ascii_case("feb") => Month::February,
        x if x.eq_ignore_ascii_case("mar") => Month::March,
        x if x.eq_ignore_ascii_case("apr") => Month::April,
        x if x.eq_ignore_ascii_case("may") => Month::May,
        x if x.eq_ignore_ascii_case("jun") => Month::June,
        x if x.eq_ignore_ascii_case("jul") => Month::July,
        x if x.eq_ignore_ascii_case("aug") => Month::August,
        x if x.eq_ignore_ascii_case("sep") => Month::September,
        x if x.eq_ignore_ascii_case("oct") => Month::October,
        x if x.eq_ignore_ascii_case("nov") => Month::November,
        x if x.eq_ignore_ascii_case("dec") => Month::December,
        _ => return None,
    })
}

fn parse_hms(s: &str) -> Option<(u8, u8, u8)> {
    let mut it = s.split(':');
    let h: u8 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    let sec: u8 = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((h, m, sec))
}

fn parse_offset(s: &str) -> Option<UtcOffset> {
    let bytes = s.as_bytes();
    if bytes.len() != 5 {
        return None;
    }
    let sign: i8 = match bytes[0] {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let hh: i8 = std::str::from_utf8(&bytes[1..3]).ok()?.parse().ok()?;
    let mm: i8 = std::str::from_utf8(&bytes[3..5]).ok()?.parse().ok()?;
    UtcOffset::from_hms(sign * hh, sign * mm, 0).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_ctime_envelope_parses() {
        let raw = b"From god@heaven.af.mil Sat Jan  3 01:05:34 1996\n\
            Message 1\n\
            \n\
            From cras@irccrew.org  Tue Jul 23 19:39:23 2002\n\
            Message 2\n\
            \n";
        let mut it = MessageIterator::new(&raw[..]);
        let m1 = it.next().unwrap().unwrap();
        assert_eq!(m1.envelope_from(), "god@heaven.af.mil");
        assert_eq!(m1.internal_date(), 820631134);
        assert_eq!(m1.contents(), b"Message 1\n\n");
        let m2 = it.next().unwrap().unwrap();
        assert_eq!(m2.envelope_from(), "cras@irccrew.org");
        assert_eq!(m2.internal_date(), 1027453163);
        assert!(it.next().is_none());
    }

    #[test]
    fn mboxrd_unescape_recovers_lines() {
        let raw = b"From a@b Tue Aug  6 13:34:34 2002\n\
            Message 3\n\
            >From hello\n\
            >>From world\n\
            >>>From test\n\
            \n";
        let m = MessageIterator::new(&raw[..]).next().unwrap().unwrap();
        assert_eq!(
            m.contents(),
            b"Message 3\nFrom hello\n>From world\n>>From test\n\n"
        );
    }

    #[test]
    fn mboxrd_leaves_non_from_lines_starting_with_gt_alone() {
        let raw = b"From a@b Mon Jan 15  15:30:00  2018\n\
            Message 4\n\
            > From\n\
            >F\n";
        let m = MessageIterator::new(&raw[..]).next().unwrap().unwrap();
        assert_eq!(m.contents(), b"Message 4\n> From\n>F\n");
    }

    #[test]
    fn takeout_envelope_with_utc_offset_parses() {
        let raw = b"From 1848707889910060830@xxx Thu Nov 13 20:20:33 +0000 2025\n\
            X-GM-THRID: 1848707889910060830\n\
            X-Gmail-Labels: Inbox,Opened\n\
            Subject: hi\n\
            \n\
            body\n";
        let m = MessageIterator::new(&raw[..]).next().unwrap().unwrap();
        assert_eq!(m.envelope_from(), "1848707889910060830@xxx");
        assert_eq!(m.internal_date(), 1763065233);
        assert!(m.contents().starts_with(b"X-GM-THRID:"));
    }

    #[test]
    fn takeout_envelope_with_negative_offset_applies_offset() {
        let raw = b"From x@y Thu Nov 13 12:20:33 -0800 2025\n\
            body\n";
        let m = MessageIterator::new(&raw[..]).next().unwrap().unwrap();
        assert_eq!(m.internal_date(), 1763065233);
    }

    #[test]
    fn malformed_envelope_date_yields_zero_not_error() {
        let raw = b"From who knows what\nbody\n";
        let m = MessageIterator::new(&raw[..]).next().unwrap().unwrap();
        assert_eq!(m.internal_date(), 0);
    }

    #[test]
    fn streams_multiple_takeout_format_messages() {
        let raw = b"From 1@xxx Thu Nov 13 20:20:33 +0000 2025\n\
            X-GM-THRID: 1\n\
            X-Gmail-Labels: Inbox\n\
            Subject: A\n\
            \n\
            body a\n\
            From 2@xxx Thu Nov 13 21:00:00 +0000 2025\n\
            X-GM-THRID: 2\n\
            X-Gmail-Labels: Sent,Archived\n\
            Subject: B\n\
            \n\
            body b\n";
        let parsed: Vec<_> = MessageIterator::new(&raw[..])
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].envelope_from(), "1@xxx");
        assert_eq!(parsed[1].envelope_from(), "2@xxx");
        assert!(parsed[0].contents().starts_with(b"X-GM-THRID: 1"));
    }
}
