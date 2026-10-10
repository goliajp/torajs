//! The kind test behind an annotated `JSON.parse` whose typed parse
//! stopped: the declaration re-parses the text on the any lane and
//! walks the value tree against its annotation before it builds the
//! annotated value. The walk itself is compiled from the static type;
//! this answers the one question the compiler cannot spell inline,
//! which kind of JSON value a node is.

use torajs_rc::Tag;

use crate::nanbox::{AnyValue, as_void_ptr, is_bool, is_cell, is_double, is_int32, is_short_str};

/// The JSON kinds a typed slot can hold, as the compiler passes them.
const NUMBER: i64 = 0;
const BOOLEAN: i64 = 1;
const STRING: i64 = 2;
const OBJECT: i64 = 3;
const ARRAY: i64 = 4;

/// 1 when `v` is a JSON value of `kind` (0 number, 1 boolean,
/// 2 string, 3 object, 4 array), else 0. `null` is none of them: a
/// typed slot of any of these kinds does not hold it.
///
/// # Safety
/// A cell payload of `v` is a live heap cell.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_json_value_is(v: AnyValue, kind: i64) -> i64 {
    let fits = match kind {
        NUMBER => is_int32(v) || is_double(v),
        BOOLEAN => is_bool(v),
        STRING => is_short_str(v) || unsafe { cell_is(v, Tag::Str) },
        OBJECT => unsafe { cell_is(v, Tag::DynObj) },
        ARRAY => unsafe { cell_is(v, Tag::Arr) },
        _ => false,
    };
    i64::from(fits)
}

/// `v` is a heap cell whose header carries `tag`.
///
/// # Safety
/// As [`__torajs_json_value_is`].
unsafe fn cell_is(v: AnyValue, tag: Tag) -> bool {
    is_cell(v) && unsafe { (as_void_ptr(v) as *const u8).add(4).cast::<u16>().read() } == tag as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nanbox::{VALUE_FALSE, VALUE_NULL, VALUE_UNDEFINED, box_double, box_int32};

    #[test]
    fn numbers_are_numbers_only() {
        unsafe {
            assert_eq!(__torajs_json_value_is(box_int32(3), NUMBER), 1);
            assert_eq!(__torajs_json_value_is(box_double(2.5), NUMBER), 1);
            assert_eq!(__torajs_json_value_is(box_double(2.5), BOOLEAN), 0);
            assert_eq!(__torajs_json_value_is(box_int32(3), STRING), 0);
        }
    }

    #[test]
    fn null_and_undefined_fit_no_kind() {
        for kind in [NUMBER, BOOLEAN, STRING, OBJECT, ARRAY] {
            assert_eq!(unsafe { __torajs_json_value_is(VALUE_NULL, kind) }, 0);
            assert_eq!(unsafe { __torajs_json_value_is(VALUE_UNDEFINED, kind) }, 0);
        }
    }

    #[test]
    fn booleans_are_booleans_only() {
        unsafe {
            assert_eq!(__torajs_json_value_is(VALUE_FALSE, BOOLEAN), 1);
            assert_eq!(__torajs_json_value_is(VALUE_FALSE, NUMBER), 0);
            assert_eq!(__torajs_json_value_is(VALUE_FALSE, OBJECT), 0);
        }
    }
}
