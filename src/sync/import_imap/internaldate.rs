/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use crate::error::Error;

pub fn imap_internaldate_to_rfc3339(s: &str) -> Result<String, Error> {
    let s = s.trim_start_matches(' ').trim_end();
    let (day, rest) = s
        .split_once('-')
        .ok_or_else(|| Error::Partial(format!("INTERNALDATE: missing day in {s:?}")))?;
    let (mon, rest) = rest
        .split_once('-')
        .ok_or_else(|| Error::Partial(format!("INTERNALDATE: missing month in {s:?}")))?;
    let (year, rest) = rest
        .split_once(' ')
        .ok_or_else(|| Error::Partial(format!("INTERNALDATE: missing year in {s:?}")))?;
    let (time, zone) = rest
        .split_once(' ')
        .ok_or_else(|| Error::Partial(format!("INTERNALDATE: missing zone in {s:?}")))?;
    let (h, ms) = time
        .split_once(':')
        .ok_or_else(|| Error::Partial(format!("INTERNALDATE: bad time {time:?}")))?;
    let (m, sec) = ms
        .split_once(':')
        .ok_or_else(|| Error::Partial(format!("INTERNALDATE: bad time {time:?}")))?;
    let day: u32 = day
        .trim()
        .parse()
        .map_err(|e| Error::Partial(format!("INTERNALDATE day {day:?}: {e}")))?;
    let mon_idx = month_to_num(mon)?;
    let year: i32 = year
        .parse()
        .map_err(|e| Error::Partial(format!("INTERNALDATE year {year:?}: {e}")))?;
    let h: u32 = h
        .parse()
        .map_err(|e| Error::Partial(format!("INTERNALDATE hour {h:?}: {e}")))?;
    let m: u32 = m
        .parse()
        .map_err(|e| Error::Partial(format!("INTERNALDATE minute {m:?}: {e}")))?;
    let sec: u32 = sec
        .parse()
        .map_err(|e| Error::Partial(format!("INTERNALDATE second {sec:?}: {e}")))?;
    let (sign, hh, mm) = parse_zone(zone)?;
    let mut out = String::with_capacity(32);
    use std::fmt::Write;
    let _ = write!(
        &mut out,
        "{year:04}-{mon_idx:02}-{day:02}T{h:02}:{m:02}:{sec:02}"
    );
    if hh == 0 && mm == 0 {
        out.push('Z');
    } else {
        let _ = write!(&mut out, "{sign}{hh:02}:{mm:02}");
    }
    Ok(out)
}

fn month_to_num(s: &str) -> Result<u32, Error> {
    let m = match s {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        other => return Err(Error::Partial(format!("INTERNALDATE month {other:?}"))),
    };
    Ok(m)
}

fn parse_zone(s: &str) -> Result<(char, u32, u32), Error> {
    let s = s.trim();
    if s.len() != 5 {
        return Err(Error::Partial(format!("INTERNALDATE zone {s:?}")));
    }
    let sign = s.as_bytes()[0] as char;
    if sign != '+' && sign != '-' {
        return Err(Error::Partial(format!("INTERNALDATE zone sign {sign}")));
    }
    let hh: u32 = s[1..3]
        .parse()
        .map_err(|e| Error::Partial(format!("INTERNALDATE zone hour: {e}")))?;
    let mm: u32 = s[3..5]
        .parse()
        .map_err(|e| Error::Partial(format!("INTERNALDATE zone min: {e}")))?;
    Ok((sign, hh, mm))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_zone_becomes_z() {
        assert_eq!(
            imap_internaldate_to_rfc3339("17-Jul-1996 02:44:25 +0000").unwrap(),
            "1996-07-17T02:44:25Z"
        );
    }

    #[test]
    fn positive_offset_preserved() {
        assert_eq!(
            imap_internaldate_to_rfc3339("17-Jul-1996 02:44:25 +0200").unwrap(),
            "1996-07-17T02:44:25+02:00"
        );
    }

    #[test]
    fn negative_offset_preserved() {
        assert_eq!(
            imap_internaldate_to_rfc3339("17-Jul-1996 02:44:25 -0700").unwrap(),
            "1996-07-17T02:44:25-07:00"
        );
    }

    #[test]
    fn single_digit_day_padded() {
        assert_eq!(
            imap_internaldate_to_rfc3339("5-Jan-2024 09:05:01 +0000").unwrap(),
            "2024-01-05T09:05:01Z"
        );
    }

    #[test]
    fn leading_space_single_digit_day() {
        assert_eq!(
            imap_internaldate_to_rfc3339(" 5-Jan-2024 09:05:01 +0000").unwrap(),
            "2024-01-05T09:05:01Z"
        );
    }

    #[test]
    fn invalid_month_errors() {
        assert!(imap_internaldate_to_rfc3339("1-XYZ-2024 00:00:00 +0000").is_err());
    }

    #[test]
    fn malformed_input_errors() {
        assert!(imap_internaldate_to_rfc3339("garbage").is_err());
        assert!(imap_internaldate_to_rfc3339("17-Jul-1996").is_err());
    }
}
