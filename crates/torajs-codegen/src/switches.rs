//! The allocator's debug and bisect switches, read from the
//! environment once per process: each function's allocation used to
//! read them again, a few environment lookups per function that a
//! module of thousands of small functions paid for thousands of times.

use std::sync::OnceLock;

pub(crate) struct Switches {
    /// `TORAJS_COALESCE_OFF=1`: no copy coalescing (bisect gate).
    pub(crate) coalesce_off: bool,
    /// `TORAJS_COALESCE_LIVE_OFF=1`: no merging of a copy with its
    /// source while both are live (bisect gate).
    pub(crate) coalesce_live_off: bool,
    /// `TORAJS_SHARE_OFF=1`: no register shared across holes (bisect
    /// gate).
    pub(crate) share_off: bool,
    /// `TORAJS_CALL_SAVE_OFF=1`: no caller-saved register saved around
    /// calls for a call-crossing value (bisect gate).
    pub(crate) call_save_off: bool,
    /// `TORAJS_REGALLOC_POOL=<n>`: registers kept per pool.
    pub(crate) pool: usize,
    /// `TORAJS_REGALLOC_DUMP=<substring>`: the functions to narrate.
    pub(crate) dump: Option<String>,
}

fn is_1(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| v == "1")
}

pub(crate) fn get() -> &'static Switches {
    static SWITCHES: OnceLock<Switches> = OnceLock::new();
    SWITCHES.get_or_init(|| Switches {
        coalesce_off: is_1("TORAJS_COALESCE_OFF"),
        coalesce_live_off: is_1("TORAJS_COALESCE_LIVE_OFF"),
        share_off: is_1("TORAJS_SHARE_OFF"),
        call_save_off: is_1("TORAJS_CALL_SAVE_OFF"),
        pool: std::env::var("TORAJS_REGALLOC_POOL")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(usize::MAX),
        dump: std::env::var("TORAJS_REGALLOC_DUMP")
            .ok()
            .filter(|s| !s.is_empty()),
    })
}
