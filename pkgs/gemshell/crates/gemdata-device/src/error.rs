//! Exit-code-carrying error type.
//!
//! The migrated shell scripts defined explicit exit codes where a caller
//! (or another script) depends on them:
//!   backlight  0 ok / 1 usage / 2 /dev/mem / 3 bad value
//!   battstat   0 ok / 2 low / 3 USB-not-charging / 4 critical / 5 no psy
//!   cl2-down   2 usage (speaker: 2 usage)
//! Everywhere else failure is exit 1. `Cerr` carries the code so a
//! subcommand handler can produce the script-compatible status, and
//! `main` maps it to the process exit code.

use std::fmt;

#[derive(Debug, Clone)]
pub struct Cerr {
    pub code: i32,
    pub msg: String,
}

impl Cerr {
    /// Build an error carrying a specific process exit code.
    pub fn new(code: i32, msg: impl Into<String>) -> Self {
        Cerr { code, msg: msg.into() }
    }

    /// Generic failure (exit 1).
    pub fn msg(msg: impl Into<String>) -> Self {
        Cerr::new(1, msg)
    }

    /// Re-stamp the exit code (e.g. backlight maps a devmem failure to 2).
    pub fn recode(mut self, code: i32) -> Self {
        self.code = code;
        self
    }
}

impl fmt::Display for Cerr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.msg)
    }
}

impl std::error::Error for Cerr {}

pub type Res<T> = Result<T, Cerr>;

/// Convenience constructor.
pub fn cerr(code: i32, msg: impl Into<String>) -> Cerr {
    Cerr::new(code, msg)
}

/// Convenience constructor for generic (exit 1) failures.
pub fn cmsg(msg: impl Into<String>) -> Cerr {
    Cerr::msg(msg)
}
