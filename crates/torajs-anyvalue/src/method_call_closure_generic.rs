//! Borrowed-builtin generic lanes, carved out of
//! `method_call_closure.rs` when the 420-06 class-ctor arm pushed it
//! past the 500-line bar: the family-generic re-dispatch gate every
//! borrowed-builtin station runs (`generic_builtin_this`) and the
//! §22.1.3 generic ToString(this) coerce (`generic_str_this`),
//! together with their receiver-shape predicates.

use core::ffi::c_void;

use torajs_rc::{
    ANY_METHOD_AT, ANY_METHOD_CONCAT, ANY_METHOD_INCLUDES, ANY_METHOD_INDEX_OF,
    ANY_METHOD_LAST_INDEX_OF, ANY_METHOD_SLICE, ANY_METHOD_TO_LOCALE_STRING, ANY_METHOD_TO_STRING,
    ANY_METHOD_VALUE_OF, Tag,
};

use crate::nanbox::{
    AnyValue, VALUE_UNDEFINED, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_null,
    is_short_str, is_undefined,
};

unsafe extern "C" {
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_str_drop(s: *mut c_void);
}

/// §22.1.3 "the String.prototype methods are generic" — a reified
/// String.prototype method reached through `.call` / `.apply` with a
/// non-string thisArg runs ToString(this) (full OrdinaryToPrimitive,
/// observable toString→valueOf order; a double-object receiver
/// leaves a pending TypeError for the caller's throw check) and
/// dispatches the Str arm on the coerced temp. Excluded from the
/// coerce, staying on the ordinary lane:
/// - `toString` / `valueOf` (thisStringValue §22.1.3.28/.35) and
///   `toLocaleString` — a non-String receiver is a TypeError there;
/// - unless `str_family` (the cell was minted for the String
///   prototype — RFC 20260721 G4 per-family cells), mids SHARED
///   with the Array surface (at / concat / includes / indexOf /
///   lastIndexOf / slice): a family-less cell's
///   `Array.prototype.indexOf.call(arrayLike)` re-dispatch must
///   reach the array-like generic arm;
/// - string-shaped and nullish receivers (identity fast paths /
///   RequireObjectCoercible throw).
/// The family-generic re-dispatch gate every borrowed-builtin
/// station runs before the ordinary receiver-arm redispatch:
///
/// - An Array-prototype-minted `concat` on a primitive receiver
///   seeds `ToObject(this)` per §23.1.3.1 (the receiver arms only
///   know their own-family concat — string concat / no-such);
/// - a String-prototype-minted cell runs the §22.1.3 generic
///   ToString(this) lane ([`generic_str_this`]).
pub(crate) unsafe fn generic_builtin_this(
    mid: i64,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
    fam: i64,
) -> Option<AnyValue> {
    // §20.2.3.5 Function.prototype.toString on a CLASS constructor —
    // tr models the class object as a dynobj (FLAG_DYNOBJ_CLASS_CTOR),
    // so no station had an fn_addr to resolve and the answer fell
    // through empty. Every borrowed-builtin invoke funnels through
    // this entry — `C.toString()` after its own-property probe
    // missed, `Function.prototype.toString.call(C)`, and the
    // ToPrimitive method dispatch — while a monkey-patched own
    // `toString` was taken before the builtin cell resolved and
    // never reaches here. Classes with no recorded source
    // (lib/eval-injected) miss the table and keep their fallthrough.
    // `valueOf` rides along: %Function.prototype% owns none, so the
    // inherited §20.1.4.7 Object.prototype.valueOf identity applies —
    // the pre-existing redispatch looped into not_callable instead,
    // killing `"" + C` (ToPrimitive default hint runs valueOf first).
    if is_cell(this_arg)
        && let Some(out) = unsafe { crate::method_value_class::class_ctor_method(mid, this_arg) }
    {
        return Some(out);
    }
    if fam == crate::method_value::family::ARR_PROTO_FAMILY
        && mid == ANY_METHOD_CONCAT
        && is_prim_shaped(this_arg)
    {
        return Some(unsafe {
            crate::method_call_arraylike_concat::prim_method(this_arg, argv, argc)
        });
    }
    // §24.1.3 / §24.2.3 / §24.3.3 / §24.4.3 — every own method of the
    // Map / Set / WeakMap / WeakSet prototypes brand-checks its
    // receiver's internal slot ([[MapData]] and kin). The ordinary
    // re-dispatch routes by RECEIVER family, so a cell minted for one
    // of these prototypes rebound onto another collection ran the
    // receiver's semantics silently (405-06:
    // `WeakMap.prototype.getOrInsert.call(new Map(), k, v)` upserted
    // into the Map). Inherited mids intern to the Object row, so
    // borrowed Object.prototype surface never trips this.
    let brand_tag = match fam {
        11 => Some(Tag::Map as u16),
        12 => Some(Tag::Set as u16),
        16 => Some(Tag::WeakMap as u16),
        17 => Some(Tag::WeakSet as u16),
        _ => None,
    };
    if let Some(want) = brand_tag {
        let ok = is_cell(this_arg)
            && unsafe { (as_void_ptr(this_arg).cast::<u8>().add(4) as *const u16).read() } == want;
        if !ok {
            unsafe {
                __torajs_throw_type_error(
                    c"builtin prototype method requires |this| to match its brand".as_ptr(),
                );
            }
            return Some(VALUE_UNDEFINED);
        }
    }
    // §21.1.3 thisNumberValue / §20.3.3 thisBooleanValue — a Number-
    // or Boolean-prototype-minted toString / valueOf borrowed onto a
    // receiver of the wrong brand is a TypeError (rotation 204,
    // mirror of the String family's thisStringValue gate below;
    // toString is NOT generic for these prototypes).
    if matches!(mid, ANY_METHOD_TO_STRING | ANY_METHOD_VALUE_OF) {
        let wrong_brand = (fam == crate::method_value::NUM_PROTO_FAMILY
            && !is_number_shaped(this_arg))
            || (fam == crate::method_value::BOOL_PROTO_FAMILY && !is_boolean_shaped(this_arg));
        if wrong_brand {
            unsafe {
                __torajs_throw_type_error(
                    c"builtin prototype method requires |this| to match its brand".as_ptr(),
                );
            }
            return Some(VALUE_UNDEFINED);
        }
    }
    unsafe {
        generic_str_this(
            mid,
            this_arg,
            argv,
            argc,
            fam == crate::method_value::STR_PROTO_FAMILY,
        )
    }
}

/// thisNumberValue shape — a number immediate or a Number wrapper
/// (whose thisNumberValue is its [[NumberData]]).
fn is_number_shaped(v: AnyValue) -> bool {
    if is_int32(v) || is_double(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::NumberWrapper as u16
}

/// thisBooleanValue shape — a bool immediate or a Boolean wrapper.
fn is_boolean_shaped(v: AnyValue) -> bool {
    if is_bool(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::BooleanWrapper as u16
}

/// A primitive shape whose ToObject mints a fresh wrapper — the
/// receivers whose own dispatch arm would answer the WRONG concat
/// (string concat on Str shapes, no-such on bool/number). Heap
/// receivers (wrapper objects included) already ride the cell arm's
/// seeded concat gate.
fn is_prim_shaped(v: AnyValue) -> bool {
    if is_bool(v) || is_int32(v) || is_double(v) || is_short_str(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::Str as u16
}

/// §6.1.4-shape probe for the thisStringValue gate — a ShortStr
/// immediate, a Str cell, or a String wrapper object (whose
/// thisStringValue is its [[StringData]]).
fn is_string_shaped(v: AnyValue) -> bool {
    if is_short_str(v) {
        return true;
    }
    if !is_cell(v) {
        return false;
    }
    let tag = unsafe { (as_void_ptr(v).cast::<u8>().add(4) as *const u16).read() };
    tag == Tag::Str as u16 || tag == Tag::StringWrapper as u16
}

pub(crate) unsafe fn generic_str_this(
    mid: i64,
    this_arg: AnyValue,
    argv: *const u64,
    argc: i64,
    str_family: bool,
) -> Option<AnyValue> {
    if !crate::method_support::str_supports(mid) {
        return None;
    }
    if matches!(
        mid,
        ANY_METHOD_TO_STRING | ANY_METHOD_VALUE_OF | ANY_METHOD_TO_LOCALE_STRING
    ) {
        // §22.1.3.28/.35 thisStringValue — a String-prototype-minted
        // toString / valueOf borrowed onto a non-string receiver is
        // a TypeError (RFC 20260721 G5); string shapes ride the
        // ordinary re-dispatch identity lane. toLocaleString is the
        // inherited generic (§20.1.4.6), never brand-checked here.
        if str_family && mid != ANY_METHOD_TO_LOCALE_STRING && !is_string_shaped(this_arg) {
            unsafe {
                __torajs_throw_type_error(
                    c"String.prototype method requires that |this| be a String".as_ptr(),
                );
            }
            return Some(VALUE_UNDEFINED);
        }
        return None;
    }
    if !str_family
        && matches!(
            mid,
            ANY_METHOD_AT
                | ANY_METHOD_CONCAT
                | ANY_METHOD_INCLUDES
                | ANY_METHOD_INDEX_OF
                | ANY_METHOD_LAST_INDEX_OF
                | ANY_METHOD_SLICE
        )
    {
        return None;
    }
    if is_undefined(this_arg) || is_null(this_arg) || is_short_str(this_arg) {
        return None;
    }
    if is_cell(this_arg) {
        let ptr = as_void_ptr(this_arg);
        if unsafe { (ptr.cast::<u8>().add(4) as *const u16).read() } == Tag::Str as u16 {
            return None;
        }
    }
    // §22.1.3.14 / §22.1.3.20 step 2.b precedes step 3's ToString(O)
    // — a non-global RegExp search argument disqualifies the call
    // before the receiver's user `toString` may run.
    if unsafe { crate::method_call_str::reject_non_global_regex_search(mid, argv, argc) } {
        return Some(VALUE_UNDEFINED);
    }
    unsafe {
        let s = crate::nanbox_ffi::__torajs_anyv_to_str(this_arg);
        let out = crate::method_call_str::str_method(s as *mut u8, mid, argv, argc);
        __torajs_str_drop(s);
        Some(out)
    }
}
