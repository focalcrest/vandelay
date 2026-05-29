/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use serde_json::{Map, Value, json};

use crate::exchange_ews::parse::{RawRecurrence, RecurrencePattern, RecurrenceRange};

pub fn to_jscalendar_rule(raw: &RawRecurrence) -> Option<Value> {
    let mut rule: Map<String, Value> = Map::new();
    match raw.pattern.as_ref()? {
        RecurrencePattern::Daily { interval } => {
            rule.insert("frequency".to_owned(), Value::String("daily".to_owned()));
            if *interval > 1 {
                rule.insert("interval".to_owned(), Value::from(*interval));
            }
        }
        RecurrencePattern::Weekly {
            interval,
            days_of_week,
        } => {
            rule.insert("frequency".to_owned(), Value::String("weekly".to_owned()));
            if *interval > 1 {
                rule.insert("interval".to_owned(), Value::from(*interval));
            }
            let days: Vec<Value> = days_of_week
                .iter()
                .filter_map(|d| day_token(d).map(|t| json!({"day": t})))
                .collect();
            if !days.is_empty() {
                rule.insert("byDay".to_owned(), Value::Array(days));
            }
        }
        RecurrencePattern::AbsoluteMonthly {
            interval,
            day_of_month,
        } => {
            rule.insert("frequency".to_owned(), Value::String("monthly".to_owned()));
            if *interval > 1 {
                rule.insert("interval".to_owned(), Value::from(*interval));
            }
            rule.insert(
                "byMonthDay".to_owned(),
                Value::Array(vec![Value::from(*day_of_month)]),
            );
        }
        RecurrencePattern::RelativeMonthly {
            interval,
            day_of_week_index,
            days_of_week,
        } => {
            rule.insert("frequency".to_owned(), Value::String("monthly".to_owned()));
            if *interval > 1 {
                rule.insert("interval".to_owned(), Value::from(*interval));
            }
            let nth = nth_of_period(day_of_week_index);
            let days: Vec<Value> = days_of_week
                .iter()
                .filter_map(|d| {
                    day_token(d).map(|t| {
                        let mut o = Map::new();
                        o.insert("day".to_owned(), Value::String(t.to_owned()));
                        if let Some(n) = nth {
                            o.insert("nthOfPeriod".to_owned(), Value::from(n));
                        }
                        Value::Object(o)
                    })
                })
                .collect();
            if !days.is_empty() {
                rule.insert("byDay".to_owned(), Value::Array(days));
            }
        }
        RecurrencePattern::AbsoluteYearly {
            month,
            day_of_month,
        } => {
            rule.insert("frequency".to_owned(), Value::String("yearly".to_owned()));
            if let Some(n) = month_number(month) {
                rule.insert(
                    "byMonth".to_owned(),
                    Value::Array(vec![Value::String(n.to_string())]),
                );
            }
            rule.insert(
                "byMonthDay".to_owned(),
                Value::Array(vec![Value::from(*day_of_month)]),
            );
        }
        RecurrencePattern::RelativeYearly {
            month,
            day_of_week_index,
            days_of_week,
        } => {
            rule.insert("frequency".to_owned(), Value::String("yearly".to_owned()));
            if let Some(n) = month_number(month) {
                rule.insert(
                    "byMonth".to_owned(),
                    Value::Array(vec![Value::String(n.to_string())]),
                );
            }
            let nth = nth_of_period(day_of_week_index);
            let days: Vec<Value> = days_of_week
                .iter()
                .filter_map(|d| {
                    day_token(d).map(|t| {
                        let mut o = Map::new();
                        o.insert("day".to_owned(), Value::String(t.to_owned()));
                        if let Some(n) = nth {
                            o.insert("nthOfPeriod".to_owned(), Value::from(n));
                        }
                        Value::Object(o)
                    })
                })
                .collect();
            if !days.is_empty() {
                rule.insert("byDay".to_owned(), Value::Array(days));
            }
        }
    }
    match raw.range.as_ref() {
        Some(RecurrenceRange::NoEnd { .. }) | None => {}
        Some(RecurrenceRange::EndDate { end_date, .. }) => {
            let local = if end_date.contains('T') {
                end_date.clone()
            } else {
                format!("{end_date}T23:59:59")
            };
            rule.insert("until".to_owned(), Value::String(local));
        }
        Some(RecurrenceRange::Numbered {
            number_of_occurrences,
            ..
        }) => {
            rule.insert("count".to_owned(), Value::from(*number_of_occurrences));
        }
    }
    Some(Value::Object(rule))
}

fn day_token(d: &str) -> Option<&'static str> {
    match d.to_ascii_lowercase().as_str() {
        "monday" | "mo" => Some("mo"),
        "tuesday" | "tu" => Some("tu"),
        "wednesday" | "we" => Some("we"),
        "thursday" | "th" => Some("th"),
        "friday" | "fr" => Some("fr"),
        "saturday" | "sa" => Some("sa"),
        "sunday" | "su" => Some("su"),
        _ => None,
    }
}

fn nth_of_period(index: &str) -> Option<i32> {
    match index {
        "First" => Some(1),
        "Second" => Some(2),
        "Third" => Some(3),
        "Fourth" => Some(4),
        "Last" => Some(-1),
        _ => None,
    }
}

fn month_number(m: &str) -> Option<u32> {
    Some(match m {
        "January" => 1,
        "February" => 2,
        "March" => 3,
        "April" => 4,
        "May" => 5,
        "June" => 6,
        "July" => 7,
        "August" => 8,
        "September" => 9,
        "October" => 10,
        "November" => 11,
        "December" => 12,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_interval_round_trips() {
        let raw = RawRecurrence {
            pattern: Some(RecurrencePattern::Daily { interval: 3 }),
            range: Some(RecurrenceRange::Numbered {
                start_date: "2025-01-01".to_owned(),
                number_of_occurrences: 5,
            }),
        };
        let rule = to_jscalendar_rule(&raw).unwrap();
        assert_eq!(rule["frequency"], "daily");
        assert_eq!(rule["interval"], 3);
        assert_eq!(rule["count"], 5);
    }

    #[test]
    fn relative_monthly_nth_of_period() {
        let raw = RawRecurrence {
            pattern: Some(RecurrencePattern::RelativeMonthly {
                interval: 1,
                day_of_week_index: "First".to_owned(),
                days_of_week: vec!["Monday".to_owned()],
            }),
            range: Some(RecurrenceRange::NoEnd {
                start_date: "2025-01-01".to_owned(),
            }),
        };
        let rule = to_jscalendar_rule(&raw).unwrap();
        assert_eq!(rule["frequency"], "monthly");
        let by_day = rule["byDay"].as_array().unwrap();
        assert_eq!(by_day[0]["day"], "mo");
        assert_eq!(by_day[0]["nthOfPeriod"], 1);
    }

    #[test]
    fn relative_yearly_last_friday_in_june() {
        let raw = RawRecurrence {
            pattern: Some(RecurrencePattern::RelativeYearly {
                month: "June".to_owned(),
                day_of_week_index: "Last".to_owned(),
                days_of_week: vec!["Friday".to_owned()],
            }),
            range: Some(RecurrenceRange::NoEnd {
                start_date: "2020-01-01".to_owned(),
            }),
        };
        let rule = to_jscalendar_rule(&raw).unwrap();
        assert_eq!(rule["frequency"], "yearly");
        assert_eq!(rule["byMonth"][0], "6");
        assert_eq!(rule["byDay"][0]["day"], "fr");
        assert_eq!(rule["byDay"][0]["nthOfPeriod"], -1);
    }

    #[test]
    fn absolute_monthly_translates_to_by_month_day() {
        let raw = RawRecurrence {
            pattern: Some(RecurrencePattern::AbsoluteMonthly {
                interval: 2,
                day_of_month: 15,
            }),
            range: Some(RecurrenceRange::NoEnd {
                start_date: "2025-01-15".to_owned(),
            }),
        };
        let rule = to_jscalendar_rule(&raw).unwrap();
        assert_eq!(rule["frequency"], "monthly");
        assert_eq!(rule["interval"], 2);
        assert_eq!(rule["byMonthDay"][0], 15);
    }

    #[test]
    fn absolute_yearly_translates_to_by_month_and_day() {
        let raw = RawRecurrence {
            pattern: Some(RecurrencePattern::AbsoluteYearly {
                month: "January".to_owned(),
                day_of_month: 1,
            }),
            range: Some(RecurrenceRange::NoEnd {
                start_date: "2025-01-01".to_owned(),
            }),
        };
        let rule = to_jscalendar_rule(&raw).unwrap();
        assert_eq!(rule["frequency"], "yearly");
        assert_eq!(rule["byMonth"][0], "1");
        assert_eq!(rule["byMonthDay"][0], 1);
    }

    #[test]
    fn no_end_range_emits_no_until_or_count() {
        let raw = RawRecurrence {
            pattern: Some(RecurrencePattern::Daily { interval: 1 }),
            range: Some(RecurrenceRange::NoEnd {
                start_date: "2025-01-01".to_owned(),
            }),
        };
        let rule = to_jscalendar_rule(&raw).unwrap();
        assert!(rule.get("until").is_none());
        assert!(rule.get("count").is_none());
    }

    #[test]
    fn weekly_two_days() {
        let raw = RawRecurrence {
            pattern: Some(RecurrencePattern::Weekly {
                interval: 1,
                days_of_week: vec!["Monday".to_owned(), "Wednesday".to_owned()],
            }),
            range: Some(RecurrenceRange::EndDate {
                start_date: "2025-01-06".to_owned(),
                end_date: "2025-06-30".to_owned(),
            }),
        };
        let rule = to_jscalendar_rule(&raw).unwrap();
        assert_eq!(rule["frequency"], "weekly");
        assert_eq!(rule["until"], "2025-06-30T23:59:59");
        let by_day = rule["byDay"].as_array().unwrap();
        assert_eq!(by_day.len(), 2);
        assert_eq!(by_day[0]["day"], "mo");
        assert_eq!(by_day[1]["day"], "we");
    }
}
