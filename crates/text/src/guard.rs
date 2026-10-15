//! A safety net around the shaping library.
//!
//! Parley is young and, fed hostile text, has been seen to panic on internal assertions. A text engine inside an editor
//! must not take the app down for a stray combining mark, so every call that hands user text to parley goes through
//! [`guarded`]: a panic is caught, and the caller falls back to a simpler version of the same work.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `f`, returning `None` if it panicked.
pub(crate) fn guarded<T>(f: impl FnOnce() -> T) -> Option<T> {
    catch_unwind(AssertUnwindSafe(f)).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panics_become_none() {
        assert_eq!(guarded(|| 7), Some(7));
        assert_eq!(guarded(|| -> u8 { std::panic::panic_any("boom") }), None);
    }
}
