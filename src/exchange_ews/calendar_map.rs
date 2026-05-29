/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde_json::{Map, Value, json};

use crate::exchange_ews::parse::{CalendarItemRaw, RawAttendee, RawOccurrence};
use crate::exchange_ews::recurrence::to_jscalendar_rule;
use crate::exchange_ews::tz::windows_to_iana;

pub struct EventValue {
    pub data: Value,
    pub is_draft: bool,
    pub use_default_alerts: bool,
}

pub fn to_jscalendar(raw: &CalendarItemRaw) -> EventValue {
    let mut event = Map::new();
    event.insert("@type".to_owned(), Value::String("Event".to_owned()));
    if let Some(uid) = raw.uid.as_ref() {
        event.insert("uid".to_owned(), Value::String(uid.clone()));
    } else {
        event.insert(
            "uid".to_owned(),
            Value::String(format!(
                "vandelay-ews-event-{}",
                blake3::hash(raw.id.id.as_bytes()).to_hex()
            )),
        );
    }
    if let Some(subject) = raw.subject.as_ref() {
        event.insert("title".to_owned(), Value::String(subject.clone()));
    }
    if let Some(loc) = raw.location.as_ref() {
        event.insert(
            "locations".to_owned(),
            Value::Object(map_singleton(
                "1",
                json!({"@type": "Location", "name": loc}),
            )),
        );
    }
    if let Some(body) = raw.body_text.as_ref() {
        event.insert("description".to_owned(), Value::String(body.clone()));
    } else if let Some(body) = raw.body_html.as_ref() {
        event.insert("description".to_owned(), Value::String(body.clone()));
        event.insert(
            "descriptionContentType".to_owned(),
            Value::String("text/html".to_owned()),
        );
    }
    if let Some(created) = raw.created.as_ref() {
        event.insert(
            "created".to_owned(),
            Value::String(normalise_utc_datetime(created)),
        );
    }
    if let Some(updated) = raw.last_modified.as_ref() {
        event.insert(
            "updated".to_owned(),
            Value::String(normalise_utc_datetime(updated)),
        );
    }
    if !raw.categories.is_empty() {
        let mut kw = Map::new();
        for c in &raw.categories {
            kw.insert(c.to_ascii_lowercase(), Value::Bool(true));
        }
        event.insert("keywords".to_owned(), Value::Object(kw));
    }
    let iana = raw
        .start_tz
        .as_deref()
        .map(|tz| windows_to_iana(tz).unwrap_or(tz).to_owned());
    if let Some(true) = raw.is_all_day_event {
        if let Some(start) = raw.start.as_ref() {
            let date_only = start.split('T').next().unwrap_or(start.as_str());
            event.insert(
                "start".to_owned(),
                Value::String(format!("{date_only}T00:00:00")),
            );
        }
        event.insert("showWithoutTime".to_owned(), Value::Bool(true));
        let days = match (raw.start.as_ref(), raw.end.as_ref()) {
            (Some(s), Some(e)) => all_day_span_days(s, e).max(1),
            _ => 1,
        };
        event.insert("duration".to_owned(), Value::String(format!("P{days}D")));
    } else {
        if let Some(start) = raw.start.as_ref() {
            event.insert(
                "start".to_owned(),
                Value::String(to_local_datetime_in(start, iana.as_deref())),
            );
        }
        if let (Some(start), Some(end)) = (raw.start.as_ref(), raw.end.as_ref())
            && let Some(dur) = duration_iso8601(start, end)
        {
            event.insert("duration".to_owned(), Value::String(dur));
        }
        if let Some(tz) = iana.as_deref() {
            event.insert("timeZone".to_owned(), Value::String(tz.to_owned()));
        }
    }
    if let Some(status) = raw.legacy_free_busy_status.as_ref() {
        let mapped = match status.as_str() {
            "Free" => "free",
            _ => "busy",
        };
        event.insert(
            "freeBusyStatus".to_owned(),
            Value::String(mapped.to_owned()),
        );
    }
    let mut participants = Map::new();
    let mut next_id = 1;
    if let Some(email) = raw.organizer_smtp.as_ref() {
        let key = next_id.to_string();
        next_id += 1;
        let cal_addr = format!("mailto:{email}");
        event.insert(
            "organizerCalendarAddress".to_owned(),
            Value::String(cal_addr.clone()),
        );
        let mut p = Map::new();
        p.insert("@type".to_owned(), Value::String("Participant".to_owned()));
        p.insert("calendarAddress".to_owned(), Value::String(cal_addr));
        p.insert("email".to_owned(), Value::String(email.clone()));
        let mut roles = Map::new();
        roles.insert("owner".to_owned(), Value::Bool(true));
        roles.insert("chair".to_owned(), Value::Bool(true));
        p.insert("roles".to_owned(), Value::Object(roles));
        if let Some(name) = raw.organizer_name.as_ref() {
            p.insert("name".to_owned(), Value::String(name.clone()));
        }
        participants.insert(key, Value::Object(p));
    }
    add_attendees(
        &mut participants,
        &mut next_id,
        &raw.required_attendees,
        true,
    );
    add_attendees(
        &mut participants,
        &mut next_id,
        &raw.optional_attendees,
        false,
    );
    if !participants.is_empty() {
        event.insert("participants".to_owned(), Value::Object(participants));
    }
    if let Some(rec) = raw.recurrence.as_ref()
        && let Some(rule) = to_jscalendar_rule(rec)
    {
        event.insert("recurrenceRules".to_owned(), Value::Array(vec![rule]));
    }
    let overrides = build_recurrence_overrides(
        &raw.modified_occurrences,
        &raw.deleted_occurrences,
        iana.as_deref(),
    );
    if let Some(overrides) = overrides {
        event.insert("recurrenceOverrides".to_owned(), overrides);
    }
    EventValue {
        data: Value::Object(event),
        is_draft: false,
        use_default_alerts: false,
    }
}

fn add_attendees(
    out: &mut Map<String, Value>,
    next_id: &mut u32,
    attendees: &[RawAttendee],
    required: bool,
) {
    for att in attendees {
        let key = next_id.to_string();
        *next_id += 1;
        let mut p = Map::new();
        p.insert("@type".to_owned(), Value::String("Participant".to_owned()));
        if let Some(email) = att.email.as_ref() {
            p.insert(
                "calendarAddress".to_owned(),
                Value::String(format!("mailto:{email}")),
            );
            p.insert("email".to_owned(), Value::String(email.clone()));
        }
        if let Some(name) = att.name.as_ref() {
            p.insert("name".to_owned(), Value::String(name.clone()));
        }
        let mut roles = Map::new();
        if required {
            roles.insert("required".to_owned(), Value::Bool(true));
        } else {
            roles.insert("optional".to_owned(), Value::Bool(true));
        }
        p.insert("roles".to_owned(), Value::Object(roles));
        if let Some(rt) = att.response_type.as_ref() {
            let mapped = match rt.as_str() {
                "Accept" => "accepted",
                "Tentative" => "tentative",
                "Decline" => "declined",
                "Organizer" => "accepted",
                _ => "needs-action",
            };
            p.insert(
                "participationStatus".to_owned(),
                Value::String(mapped.to_owned()),
            );
        }
        out.insert(key, Value::Object(p));
    }
}

fn build_recurrence_overrides(
    modified: &[RawOccurrence],
    deleted: &[RawOccurrence],
    iana: Option<&str>,
) -> Option<Value> {
    if modified.is_empty() && deleted.is_empty() {
        return None;
    }
    let mut map = Map::new();
    for occ in modified {
        let Some(key) = occ
            .original_start
            .as_deref()
            .or(occ.start.as_deref())
            .map(|s| to_local_datetime_in(s, iana))
        else {
            continue;
        };
        let mut o = Map::new();
        if let Some(s) = occ.start.as_ref() {
            o.insert(
                "start".to_owned(),
                Value::String(to_local_datetime_in(s, iana)),
            );
        }
        if let (Some(s), Some(e)) = (occ.start.as_ref(), occ.end.as_ref())
            && let Some(dur) = duration_iso8601(s, e)
        {
            o.insert("duration".to_owned(), Value::String(dur));
        }
        map.insert(key, Value::Object(o));
    }
    for occ in deleted {
        let Some(key) = occ.start.as_deref().map(|s| to_local_datetime_in(s, iana)) else {
            continue;
        };
        map.insert(key, json!({"excluded": true}));
    }
    if map.is_empty() {
        None
    } else {
        Some(Value::Object(map))
    }
}

fn normalise_utc_datetime(s: &str) -> String {
    let trimmed = s.trim();
    let stripped = trimmed
        .strip_suffix('Z')
        .or_else(|| {
            trimmed
                .rfind(['+', '-'])
                .filter(|i| *i >= 10)
                .map(|i| &trimmed[..i])
        })
        .unwrap_or(trimmed);
    let (date, time) = match stripped.split_once('T') {
        Some((d, t)) => (d, t),
        None => return format!("{stripped}T00:00:00Z"),
    };
    let time_clean = if let Some(dot) = time.find('.') {
        &time[..dot]
    } else {
        time
    };
    format!("{date}T{time_clean}Z")
}

fn to_local_datetime_in(s: &str, iana: Option<&str>) -> String {
    let trimmed = s.trim();
    let utc_anchored =
        trimmed.ends_with('Z') || trimmed.rfind(['+', '-']).filter(|i| *i >= 10).is_some();
    if utc_anchored
        && let Some(tz) = iana
        && let Some(out) = convert_utc_to_local(trimmed, tz)
    {
        return out;
    }
    let stripped = trimmed
        .strip_suffix('Z')
        .or_else(|| {
            trimmed
                .rfind(['+', '-'])
                .filter(|i| *i >= 10)
                .map(|i| &trimmed[..i])
        })
        .unwrap_or(trimmed);
    let (date, time) = match stripped.split_once('T') {
        Some((d, t)) => (d, t),
        None => return format!("{stripped}T00:00:00"),
    };
    let time_clean = if let Some(dot) = time.find('.') {
        &time[..dot]
    } else {
        time
    };
    format!("{date}T{time_clean}")
}

fn convert_utc_to_local(s: &str, iana: &str) -> Option<String> {
    use chrono::DateTime;
    let utc: DateTime<chrono::Utc> = DateTime::parse_from_rfc3339(s)
        .ok()?
        .with_timezone(&chrono::Utc);
    let tz: chrono_tz::Tz = iana.parse().ok()?;
    let local = utc.with_timezone(&tz);
    Some(local.format("%Y-%m-%dT%H:%M:%S").to_string())
}

fn all_day_span_days(start: &str, end: &str) -> u32 {
    fn date_only(s: &str) -> &str {
        s.split('T').next().unwrap_or(s)
    }
    fn ymd(s: &str) -> Option<(i64, u32, u32)> {
        let mut p = s.split('-');
        let y: i64 = p.next()?.parse().ok()?;
        let m: u32 = p.next()?.parse().ok()?;
        let d: u32 = p.next()?.parse().ok()?;
        Some((y, m, d))
    }
    fn to_days(y: i64, m: u32, d: u32) -> i64 {
        let (mut y, m) = if m <= 2 {
            (y - 1, m as i64 + 12)
        } else {
            (y, m as i64)
        };
        y += 4800;
        let mm = m + 1;
        365 * y + y / 4 - y / 100 + y / 400 + 30 * mm + 3 * (mm + 1) / 5 + d as i64 - 32045
    }
    let (Some(s), Some(e)) = (ymd(date_only(start)), ymd(date_only(end))) else {
        return 1;
    };
    let diff = to_days(e.0, e.1, e.2) - to_days(s.0, s.1, s.2);
    if diff <= 0 { 1 } else { diff as u32 }
}

fn duration_iso8601(start: &str, end: &str) -> Option<String> {
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;
    let s = OffsetDateTime::parse(start, &Rfc3339).ok()?;
    let e = OffsetDateTime::parse(end, &Rfc3339).ok()?;
    let dur = e - s;
    if dur.is_zero() {
        return Some("PT0S".to_owned());
    }
    let total_seconds = dur.whole_seconds();
    if total_seconds < 0 {
        return None;
    }
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    let mut out = String::from("PT");
    if hours > 0 {
        out.push_str(&format!("{hours}H"));
    }
    if minutes > 0 {
        out.push_str(&format!("{minutes}M"));
    }
    if seconds > 0 || (hours == 0 && minutes == 0) {
        out.push_str(&format!("{seconds}S"));
    }
    Some(out)
}

fn map_singleton(key: &str, value: Value) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert(key.to_owned(), value);
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange_ews::parse::{RecurrencePattern, RecurrenceRange};

    #[test]
    fn simple_event_round_trip_converts_utc_to_local_wall_clock() {
        let raw = CalendarItemRaw {
            uid: Some("uid-1".to_owned()),
            subject: Some("Meeting".to_owned()),
            start: Some("2025-06-15T14:00:00Z".to_owned()),
            end: Some("2025-06-15T15:00:00Z".to_owned()),
            location: Some("HQ".to_owned()),
            start_tz: Some("Pacific Standard Time".to_owned()),
            ..CalendarItemRaw::default()
        };
        let v = to_jscalendar(&raw).data;
        assert_eq!(v["@type"], "Event");
        assert_eq!(v["uid"], "uid-1");
        assert_eq!(v["title"], "Meeting");
        assert_eq!(v["start"], "2025-06-15T07:00:00");
        assert_eq!(v["duration"], "PT1H");
        assert_eq!(v["timeZone"], "America/Los_Angeles");
        assert_eq!(v["locations"]["1"]["name"], "HQ");
    }

    #[test]
    fn naive_start_without_timezone_stays_floating() {
        let raw = CalendarItemRaw {
            uid: Some("uid-naive".to_owned()),
            start: Some("2025-06-15T14:00:00".to_owned()),
            end: Some("2025-06-15T15:00:00".to_owned()),
            ..CalendarItemRaw::default()
        };
        let v = to_jscalendar(&raw).data;
        assert_eq!(v["start"], "2025-06-15T14:00:00");
        assert!(v.get("timeZone").is_none());
    }

    #[test]
    fn unknown_windows_timezone_passes_through_verbatim() {
        let raw = CalendarItemRaw {
            uid: Some("uid-x".to_owned()),
            start: Some("2025-06-15T14:00:00Z".to_owned()),
            end: Some("2025-06-15T15:00:00Z".to_owned()),
            start_tz: Some("Made Up Time".to_owned()),
            ..CalendarItemRaw::default()
        };
        let v = to_jscalendar(&raw).data;
        assert_eq!(v["start"], "2025-06-15T14:00:00");
        assert_eq!(v["timeZone"], "Made Up Time");
    }

    #[test]
    fn recurrence_override_key_uses_local_time_in_event_timezone() {
        let raw = CalendarItemRaw {
            uid: Some("uid-tz".to_owned()),
            start: Some("2025-06-15T21:00:00Z".to_owned()),
            end: Some("2025-06-15T22:00:00Z".to_owned()),
            start_tz: Some("Pacific Standard Time".to_owned()),
            recurrence: Some(crate::exchange_ews::parse::RawRecurrence {
                pattern: Some(RecurrencePattern::Daily { interval: 1 }),
                range: Some(RecurrenceRange::Numbered {
                    start_date: "2025-06-15".to_owned(),
                    number_of_occurrences: 3,
                }),
            }),
            deleted_occurrences: vec![RawOccurrence {
                item_id: Default::default(),
                start: Some("2025-06-16T21:00:00Z".to_owned()),
                end: None,
                original_start: None,
            }],
            ..CalendarItemRaw::default()
        };
        let v = to_jscalendar(&raw).data;
        let over = v["recurrenceOverrides"].as_object().unwrap();
        assert!(over.contains_key("2025-06-16T14:00:00"));
    }

    #[test]
    fn all_day_event_uses_date_only_start() {
        let raw = CalendarItemRaw {
            uid: Some("uid-2".to_owned()),
            subject: Some("Holiday".to_owned()),
            is_all_day_event: Some(true),
            start: Some("2025-12-25T00:00:00".to_owned()),
            end: Some("2025-12-26T00:00:00".to_owned()),
            ..CalendarItemRaw::default()
        };
        let v = to_jscalendar(&raw).data;
        assert_eq!(v["start"], "2025-12-25T00:00:00");
        assert_eq!(v["showWithoutTime"], true);
        assert_eq!(v["duration"], "P1D");
        assert!(v.get("timeZone").is_none());
    }

    #[test]
    fn recurring_master_with_overrides() {
        let raw = CalendarItemRaw {
            uid: Some("uid-3".to_owned()),
            start: Some("2025-06-15T14:00:00Z".to_owned()),
            end: Some("2025-06-15T15:00:00Z".to_owned()),
            recurrence: Some(crate::exchange_ews::parse::RawRecurrence {
                pattern: Some(RecurrencePattern::Daily { interval: 1 }),
                range: Some(RecurrenceRange::Numbered {
                    start_date: "2025-06-15".to_owned(),
                    number_of_occurrences: 3,
                }),
            }),
            modified_occurrences: vec![RawOccurrence {
                item_id: Default::default(),
                start: Some("2025-06-16T15:00:00Z".to_owned()),
                end: Some("2025-06-16T16:30:00Z".to_owned()),
                original_start: Some("2025-06-16T14:00:00Z".to_owned()),
            }],
            deleted_occurrences: vec![RawOccurrence {
                item_id: Default::default(),
                start: Some("2025-06-17T14:00:00Z".to_owned()),
                end: None,
                original_start: None,
            }],
            ..CalendarItemRaw::default()
        };
        let v = to_jscalendar(&raw).data;
        assert_eq!(v["recurrenceRules"][0]["frequency"], "daily");
        assert_eq!(v["recurrenceRules"][0]["count"], 3);
        let over = v["recurrenceOverrides"].as_object().unwrap();
        assert!(over.contains_key("2025-06-16T14:00:00"));
        assert_eq!(over["2025-06-17T14:00:00"]["excluded"], true);
    }

    #[test]
    fn multi_day_all_day_spans_correct_number_of_days() {
        let raw = CalendarItemRaw {
            uid: Some("uid-multi".to_owned()),
            is_all_day_event: Some(true),
            start: Some("2025-07-04T00:00:00".to_owned()),
            end: Some("2025-07-07T00:00:00".to_owned()),
            ..CalendarItemRaw::default()
        };
        let v = to_jscalendar(&raw).data;
        assert_eq!(v["duration"], "P3D");
        assert_eq!(v["showWithoutTime"], true);
    }

    #[test]
    fn attendees_get_required_or_optional_role() {
        let raw = CalendarItemRaw {
            uid: Some("uid-att".to_owned()),
            start: Some("2025-06-15T14:00:00Z".to_owned()),
            end: Some("2025-06-15T15:00:00Z".to_owned()),
            organizer_smtp: Some("alice@x".to_owned()),
            required_attendees: vec![crate::exchange_ews::parse::RawAttendee {
                email: Some("bob@x".to_owned()),
                name: None,
                response_type: Some("Accept".to_owned()),
            }],
            optional_attendees: vec![crate::exchange_ews::parse::RawAttendee {
                email: Some("eve@x".to_owned()),
                name: None,
                response_type: None,
            }],
            ..CalendarItemRaw::default()
        };
        let v = to_jscalendar(&raw).data;
        assert_eq!(v["organizerCalendarAddress"], "mailto:alice@x");
        let participants = v["participants"].as_object().unwrap();
        let required = participants
            .values()
            .find(|p| p["email"] == "bob@x")
            .unwrap();
        assert_eq!(required["roles"]["required"], true);
        assert!(required["roles"].get("optional").is_none());
        let optional = participants
            .values()
            .find(|p| p["email"] == "eve@x")
            .unwrap();
        assert_eq!(optional["roles"]["optional"], true);
    }

    #[test]
    fn all_day_span_helper_is_inclusive_of_start() {
        assert_eq!(all_day_span_days("2025-07-04", "2025-07-04"), 1);
        assert_eq!(all_day_span_days("2025-07-04", "2025-07-05"), 1);
        assert_eq!(all_day_span_days("2025-07-04", "2025-07-07"), 3);
        assert_eq!(all_day_span_days("bad", "input"), 1);
    }
}
