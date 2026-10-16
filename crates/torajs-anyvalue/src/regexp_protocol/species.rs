//! §7.3.22 SpeciesConstructor and the Construct that follows it — how
//! @@split and @@matchAll mint the RegExp they drive instead of the
//! receiver itself.

use super::ops::{Owned, OwnedStr, get, type_error};
use crate::nanbox::{AnyValue, is_null, is_undefined};

/// `WELL_KNOWN_NAMES` position of `species`.
const WK_SPECIES: i64 = 10;
/// %RegExp%'s builtin-prototype tag (its constructor row).
const REGEXP_TAG: i64 = 7;

/// §7.3.22 SpeciesConstructor(rx, %RegExp%) — `None` = abrupt,
/// `Some(None)` = the default constructor.
pub(super) unsafe fn species_constructor(rx: AnyValue) -> Option<Option<Owned>> {
    unsafe {
        let c = get(rx, b"constructor")?;
        if is_undefined(c.get()) {
            return Some(None);
        }
        if !crate::iter_zip_shared::av_is_object(c.get()) {
            type_error(c"object.constructor is not an object");
            return None;
        }
        let sym = crate::method_value::symbol_static::well_known_singleton(WK_SPECIES);
        let species = crate::member_get_symbol::symbol_key_get(c.get(), sym).into_owned_value();
        let species = Owned::adopt(species);
        if super::ops::throw_pending() {
            return None;
        }
        if is_undefined(species.get()) || is_null(species.get()) {
            return Some(None);
        }
        if !crate::construct::is_constructor(species.get()) {
            type_error(c"object.constructor[Symbol.species] is not a constructor");
            return None;
        }
        Some(Some(species))
    }
}

/// ? Construct(C, «rx, flags»).
pub(super) unsafe fn construct(
    ctor: Option<&Owned>,
    rx: AnyValue,
    flags: &OwnedStr,
) -> Option<Owned> {
    unsafe {
        let argv = [rx, flags.boxed()];
        let v = match ctor {
            Some(c) => crate::construct::__torajs_anyv_construct(c.get(), argv.as_ptr(), 2),
            None => crate::method_value::ctor::ctor_construct(REGEXP_TAG, argv.as_ptr(), 2),
        };
        let v = Owned::adopt(v);
        if super::ops::throw_pending() {
            return None;
        }
        Some(v)
    }
}
