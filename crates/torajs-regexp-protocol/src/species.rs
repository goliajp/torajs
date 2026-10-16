//! §7.3.22 SpeciesConstructor and the Construct that follows it — how
//! @@split and @@matchAll mint the RegExp they drive instead of the
//! receiver itself.

use crate::ops::{Owned, OwnedStr, get, type_error};
use torajs_anyvalue::nanbox::{AnyValue, is_null, is_undefined};
use torajs_anyvalue::regexp_support as av;

/// `WELL_KNOWN_NAMES` position of `species`.
const WK_SPECIES: i64 = 10;

/// §7.3.22 SpeciesConstructor(rx, %RegExp%) — `None` = abrupt,
/// `Some(None)` = the default constructor.
pub(crate) unsafe fn species_constructor(rx: AnyValue) -> Option<Option<Owned>> {
    unsafe {
        let c = get(rx, b"constructor")?;
        if is_undefined(c.get()) {
            return Some(None);
        }
        if !av::av_is_object(c.get()) {
            type_error(c"object.constructor is not an object");
            return None;
        }
        let sym = av::well_known_singleton(WK_SPECIES);
        let species = av::symbol_key_get(c.get(), sym).into_owned_value();
        let species = Owned::adopt(species);
        if crate::ops::throw_pending() {
            return None;
        }
        if is_undefined(species.get()) || is_null(species.get()) {
            return Some(None);
        }
        if !av::is_constructor(species.get()) {
            type_error(c"object.constructor[Symbol.species] is not a constructor");
            return None;
        }
        Some(Some(species))
    }
}

/// ? Construct(C, «rx, flags»).
pub(crate) unsafe fn construct(
    ctor: Option<&Owned>,
    rx: AnyValue,
    flags: &OwnedStr,
) -> Option<Owned> {
    unsafe {
        let argv = [rx, flags.boxed()];
        let v = match ctor {
            Some(c) => av::__torajs_anyv_construct(c.get(), argv.as_ptr(), 2),
            // %RegExp%'s [[Construct]] row is the ctor build seam
            None => crate::ctor_build::__torajs_ctor_regexp_build(rx, flags.boxed()),
        };
        let v = Owned::adopt(v);
        if crate::ops::throw_pending() {
            return None;
        }
        Some(v)
    }
}
