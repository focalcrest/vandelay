/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: Apache-2.0 OR MIT
 */

pub const LEVEL_QUIET: u8 = 0;
pub const LEVEL_DEFAULT: u8 = 1;
pub const LEVEL_PROGRESS: u8 = 2;
pub const LEVEL_METHOD: u8 = 3;
pub const LEVEL_BODIES: u8 = 4;

#[derive(Debug, Clone, Copy)]
pub struct Logger {
    level: u8,
}

impl Logger {
    pub fn new(level: u8) -> Self {
        Logger {
            level: level.min(LEVEL_BODIES),
        }
    }

    pub fn from_flags(quiet: bool, verbose: u8) -> Self {
        let level = if quiet {
            LEVEL_QUIET
        } else {
            LEVEL_DEFAULT.saturating_add(verbose)
        };
        Logger::new(level)
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    pub fn enabled(&self, level: u8) -> bool {
        self.level >= level
    }

    pub fn warn(&self, message: &str) {
        eprintln!("warning: {message}");
    }

    pub fn error(&self, message: &str) {
        eprintln!("error: {message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_is_level_zero() {
        assert_eq!(Logger::from_flags(true, 3).level(), LEVEL_QUIET);
    }

    #[test]
    fn default_is_level_one() {
        assert_eq!(Logger::from_flags(false, 0).level(), LEVEL_DEFAULT);
    }

    #[test]
    fn verbose_caps_at_four() {
        assert_eq!(Logger::from_flags(false, 9).level(), LEVEL_BODIES);
    }

    #[test]
    fn enabled_is_a_superset_scale() {
        let l = Logger::from_flags(false, 1);
        assert!(l.enabled(LEVEL_DEFAULT));
        assert!(l.enabled(LEVEL_PROGRESS));
        assert!(!l.enabled(LEVEL_METHOD));
    }
}
