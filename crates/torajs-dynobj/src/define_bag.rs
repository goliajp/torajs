//! The lazy-expando receiver chooser for `defineProperty` — split
//! out of [`crate::define`] (file-size hard limit; the parent keeps
//! the dispatch, this file keeps "which slot, and which name is not
//! the bag's").

use core::ffi::c_void;

/// Which lazy-expando slot a receiver's own defines land in, `None`
/// when this arm does not claim the receiver.
///
/// §27.2 promise instances are ordinary objects whose own defines
/// land in a lazy expando (torajs-promise `Promise::props` @ +32;
/// +24 is the callback list) — no virtual-prop seeding, a promise
/// cell has no reflected own `name` / `length`. A user `then`
/// override stored there is the §27.2.4.1.3 step 6.q-s observation
/// surface the combinators and the any-lane method dispatch consult.
///
/// Map / Set / Date / RegExp are the same shape: their entry table,
/// [[DateValue]] and compiled program are internal state carried in
/// the cell, so §24.1.6 / §21.4.4 / §22.2.6 leave them ordinary
/// objects whose whole own face is the bag. `lastIndex` is held out —
/// §22.2.4.1 keeps it in the RegExp cell itself, so a bag entry of
/// that name would be a second own property nothing reads and the
/// enumeration surfaces would list it twice. Defining it keeps the
/// answer it has always had; routing that define into the cell slot
/// is a registered gap.
///
/// # Safety
/// `key` is a live Str cell.
pub(crate) unsafe fn lazy_bag_props_off(htag: u16, key: *mut c_void) -> Option<usize> {
    match htag {
        crate::layout::TAG_PROMISE_HDR => Some(crate::layout::PROMISE_PROPS_OFF),
        crate::layout::TAG_MAP_HDR | crate::layout::TAG_SET_HDR => {
            Some(crate::layout::MAP_PROPS_OFF)
        }
        crate::layout::TAG_DATE_HDR => Some(crate::layout::DATE_PROPS_OFF),
        crate::layout::TAG_REGEXP_HDR if !unsafe { crate::layout::key_is(key, b"lastIndex") } => {
            Some(crate::layout::REGEX_PROPS_OFF)
        }
        _ => None,
    }
}
