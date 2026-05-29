/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlagAction {
    Keep(&'static str),
    Drop,
    Verbatim,
    Deleted,
}

pub fn translate_system_flag(flag: &str) -> FlagAction {
    if !flag.starts_with('\\') {
        return FlagAction::Verbatim;
    }
    match &flag[1..].to_ascii_lowercase()[..] {
        "seen" => FlagAction::Keep("$seen"),
        "flagged" => FlagAction::Keep("$flagged"),
        "answered" => FlagAction::Keep("$answered"),
        "draft" => FlagAction::Keep("$draft"),
        "recent" => FlagAction::Drop,
        "deleted" => FlagAction::Deleted,
        _ => FlagAction::Verbatim,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub keywords: Vec<String>,
    pub has_deleted_flag: bool,
}

pub fn translate_flags(imap_flags: &[String], include_deleted: bool) -> Translation {
    let mut out: Vec<String> = Vec::with_capacity(imap_flags.len());
    let mut has_deleted = false;
    for f in imap_flags {
        if f.is_empty() {
            continue;
        }
        match translate_system_flag(f) {
            FlagAction::Keep(k) => {
                if !out.iter().any(|x| x.eq_ignore_ascii_case(k)) {
                    out.push(k.to_owned());
                }
            }
            FlagAction::Drop => {}
            FlagAction::Verbatim => {
                if !out.iter().any(|x| x.eq_ignore_ascii_case(f)) {
                    out.push(f.clone());
                }
            }
            FlagAction::Deleted => {
                has_deleted = true;
                if include_deleted && !out.iter().any(|x| x.eq_ignore_ascii_case("$deleted")) {
                    out.push("$deleted".to_owned());
                }
            }
        }
    }
    Translation {
        keywords: out,
        has_deleted_flag: has_deleted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn system_flag_mapping() {
        assert!(matches!(
            translate_system_flag("\\Seen"),
            FlagAction::Keep("$seen")
        ));
        assert!(matches!(
            translate_system_flag("\\Flagged"),
            FlagAction::Keep("$flagged")
        ));
        assert!(matches!(
            translate_system_flag("\\Answered"),
            FlagAction::Keep("$answered")
        ));
        assert!(matches!(
            translate_system_flag("\\Draft"),
            FlagAction::Keep("$draft")
        ));
        assert!(matches!(
            translate_system_flag("\\Recent"),
            FlagAction::Drop
        ));
        assert!(matches!(
            translate_system_flag("\\Deleted"),
            FlagAction::Deleted
        ));
    }

    #[test]
    fn unknown_system_flag_passes_verbatim() {
        assert!(matches!(
            translate_system_flag("\\Forwarded"),
            FlagAction::Verbatim
        ));
        assert!(matches!(
            translate_system_flag("\\MDNSent"),
            FlagAction::Verbatim
        ));
    }

    #[test]
    fn custom_keywords_pass_verbatim_case() {
        let t = translate_flags(&flags(&["$Junk", "NotJunk", "MyLabel"]), false);
        assert_eq!(t.keywords, vec!["$Junk", "NotJunk", "MyLabel"]);
        assert!(!t.has_deleted_flag);
    }

    #[test]
    fn translates_seen_and_flagged() {
        let t = translate_flags(&flags(&["\\Seen", "\\Flagged"]), false);
        assert_eq!(t.keywords, vec!["$seen", "$flagged"]);
    }

    #[test]
    fn drops_recent() {
        let t = translate_flags(&flags(&["\\Seen", "\\Recent"]), false);
        assert_eq!(t.keywords, vec!["$seen"]);
    }

    #[test]
    fn deleted_flag_without_include_marks_only() {
        let t = translate_flags(&flags(&["\\Deleted", "\\Seen"]), false);
        assert!(t.has_deleted_flag);
        assert_eq!(t.keywords, vec!["$seen"]);
    }

    #[test]
    fn deleted_flag_with_include_adds_dollar_deleted() {
        let t = translate_flags(&flags(&["\\Deleted", "\\Seen"]), true);
        assert!(t.has_deleted_flag);
        assert_eq!(t.keywords, vec!["$deleted", "$seen"]);
    }

    #[test]
    fn duplicate_keywords_deduped_case_insensitively() {
        let t = translate_flags(&flags(&["mykey", "MYKEY", "MyKey"]), false);
        assert_eq!(t.keywords, vec!["mykey"]);
    }

    #[test]
    fn empty_flag_strings_ignored() {
        let t = translate_flags(&flags(&["", "\\Seen", ""]), false);
        assert_eq!(t.keywords, vec!["$seen"]);
    }

    #[test]
    fn order_preserved() {
        let t = translate_flags(
            &flags(&["MyLabel", "\\Seen", "$Important", "\\Flagged"]),
            false,
        );
        assert_eq!(
            t.keywords,
            vec!["MyLabel", "$seen", "$Important", "$flagged"]
        );
    }
}
