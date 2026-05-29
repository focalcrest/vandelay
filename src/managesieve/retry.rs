/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

use super::error::SieveError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    TransportDrop,
    Transient,
    Permanent,
    PerScriptRecoverable,
    FeatureNegotiation,
    Referral,
}

pub fn classify(err: &SieveError) -> Disposition {
    match err {
        SieveError::Io(_) => Disposition::TransportDrop,
        SieveError::Tls(_) => Disposition::Permanent,
        SieveError::Bye(_) => Disposition::TransportDrop,
        SieveError::No(no) => {
            if no.is_referral() {
                Disposition::Referral
            } else if no.is_transient() {
                Disposition::Transient
            } else {
                Disposition::Permanent
            }
        }
        SieveError::Protocol(_) | SieveError::Parse(_) | SieveError::Unsupported(_) => {
            Disposition::Permanent
        }
        SieveError::Referral(_) => Disposition::Referral,
    }
}

pub fn is_negotiation_failure(err: &SieveError) -> bool {
    match err {
        SieveError::No(no) => !no.is_transient() && !no.is_referral(),
        SieveError::Unsupported(_) => true,
        _ => false,
    }
}

pub fn script_disposition(err: &SieveError) -> Disposition {
    let base = classify(err);
    match base {
        Disposition::Permanent | Disposition::FeatureNegotiation => {
            Disposition::PerScriptRecoverable
        }
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::super::error::NoError;
    use super::*;
    use std::io::{self, ErrorKind};

    #[test]
    fn io_errors_are_transport_drop() {
        for k in [
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::BrokenPipe,
            ErrorKind::UnexpectedEof,
            ErrorKind::TimedOut,
        ] {
            let e = SieveError::Io(io::Error::new(k, "x"));
            assert_eq!(classify(&e), Disposition::TransportDrop, "{k:?}");
        }
    }

    #[test]
    fn bye_is_transport_drop() {
        assert_eq!(
            classify(&SieveError::Bye("going away".into())),
            Disposition::TransportDrop
        );
    }

    #[test]
    fn transient_no_text_marked_transient() {
        let e = SieveError::No(NoError::new("Try again later", None));
        assert_eq!(classify(&e), Disposition::Transient);
    }

    #[test]
    fn trylater_code_marked_transient() {
        let e = SieveError::No(NoError::new("x", Some("TRYLATER".into())));
        assert_eq!(classify(&e), Disposition::Transient);
    }

    #[test]
    fn referral_code_classified_as_referral() {
        let e = SieveError::No(NoError::new("x", Some("REFERRAL".into())));
        assert_eq!(classify(&e), Disposition::Referral);
    }

    #[test]
    fn nonexistent_no_is_permanent_but_per_script_demotes_it() {
        let e = SieveError::No(NoError::new("no such script", Some("NONEXISTENT".into())));
        assert_eq!(classify(&e), Disposition::Permanent);
        assert_eq!(script_disposition(&e), Disposition::PerScriptRecoverable);
    }

    #[test]
    fn negotiation_failure_matches_non_transient_no_and_unsupported() {
        assert!(is_negotiation_failure(&SieveError::Unsupported("x".into())));
        assert!(is_negotiation_failure(&SieveError::No(NoError::new(
            "no such mech",
            None
        ))));
        assert!(!is_negotiation_failure(&SieveError::No(NoError::new(
            "try again",
            None
        ))));
        assert!(!is_negotiation_failure(&SieveError::No(NoError::new(
            "x",
            Some("REFERRAL".into())
        ))));
    }
}
