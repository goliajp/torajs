//! The ECMA-402 option-reading abstract operations the
//! `toLocaleString` family's options pass through: CoerceOptionsToObject
//! (§9.2.11), GetOption (§9.2.12), GetBooleanOrStringNumberFormatOption
//! (§9.2.13), DefaultNumberOption and GetNumberOption (§9.2.14-15).
//! The value grammars checked after a read are
//! [`crate::locale_option_values`].
//!
//! The operations are generic over an [`OptionSource`] — the runtime
//! one ([`AnyOptions`]) does real [[Get]]s and coercions, which run
//! user getters, `toString` and `valueOf`; the unit tests use a
//! recording one to pin the read order. A throw from user code ends
//! the walk at once ([`Refusal::Thrown`]).

use core::ffi::{CStr, c_void};

use crate::compare::{STR_FLAG_IS_LATIN1, STR_LEN_OFF};
use crate::member_set::STR_DATA_OFF;
use crate::nanbox::{AnyValue, VALUE_UNDEFINED, as_bool, is_bool, is_null, is_undefined};
use crate::nanbox_ffi::{
    __torajs_anyv_rc_dec, __torajs_anyv_to_bool, __torajs_anyv_to_number, __torajs_anyv_to_str,
};
use torajs_rc::HeapHeader;

unsafe extern "C" {
    fn __torajs_str_alloc(src: *const u8, len: i64) -> *mut u8;
    fn __torajs_str_drop(s: *mut c_void);
    fn __torajs_throw_check() -> i64;
    fn __torajs_throw_type_error(msg: *const core::ffi::c_char);
    fn __torajs_throw_range_error(msg: *const core::ffi::c_char);
}

/// Why reading the options stopped.
#[derive(Debug, PartialEq)]
pub(crate) enum Refusal {
    /// User code threw; the throw is already pending.
    Thrown,
    /// A value the constructor rejects with a RangeError.
    Range(&'static CStr),
    /// A combination the constructor rejects with a TypeError.
    Type(&'static CStr),
}

pub(crate) type Read<T> = Result<T, Refusal>;

/// Where option values come from: [[Get]] and the three coercions.
pub(crate) trait OptionSource {
    type Value;
    fn get(&mut self, name: &'static str) -> Read<Self::Value>;
    fn is_undefined(&self, v: &Self::Value) -> bool;
    fn to_string(&mut self, v: Self::Value) -> Read<Vec<u8>>;
    fn to_number(&mut self, v: Self::Value) -> Read<f64>;
    fn to_boolean(&mut self, v: &Self::Value) -> bool;
    /// Whether the value is the Boolean `true` itself.
    fn is_true(&self, v: &Self::Value) -> bool;
}

/// GetOption, type string: absent is `None`; else ToString, a
/// RangeError when `values` is non-empty and does not list it.
pub(crate) fn get_string<S: OptionSource>(
    s: &mut S,
    name: &'static str,
    values: &[&str],
) -> Read<Option<Vec<u8>>> {
    let v = s.get(name)?;
    if s.is_undefined(&v) {
        return Ok(None);
    }
    let text = s.to_string(v)?;
    if !values.is_empty() && !values.iter().any(|w| w.as_bytes() == text.as_slice()) {
        return Err(Refusal::Range(c"Value out of range for locale option"));
    }
    Ok(Some(text))
}

/// GetOption, type boolean: absent is `None`, else ToBoolean.
pub(crate) fn get_boolean<S: OptionSource>(s: &mut S, name: &'static str) -> Read<Option<bool>> {
    let v = s.get(name)?;
    if s.is_undefined(&v) {
        return Ok(None);
    }
    Ok(Some(s.to_boolean(&v)))
}

/// DefaultNumberOption (§9.2.14): absent is `fallback`; else ToNumber,
/// a RangeError when NaN or outside `[min, max]`, else its floor.
pub(crate) fn default_number<S: OptionSource>(
    s: &mut S,
    v: S::Value,
    min: f64,
    max: f64,
    fallback: Option<f64>,
) -> Read<Option<f64>> {
    if s.is_undefined(&v) {
        return Ok(fallback);
    }
    let n = s.to_number(v)?;
    if n.is_nan() || n < min || n > max {
        return Err(Refusal::Range(c"Value out of range for locale option"));
    }
    Ok(Some(n.floor()))
}

/// GetNumberOption (§9.2.15).
pub(crate) fn get_number<S: OptionSource>(
    s: &mut S,
    name: &'static str,
    min: f64,
    max: f64,
    fallback: Option<f64>,
) -> Read<Option<f64>> {
    let v = s.get(name)?;
    default_number(s, v, min, max, fallback)
}

/// The runtime options after CoerceOptionsToObject: `undefined` stands
/// for the empty null-prototype object, so nothing is ever found on it
/// (an inherited `Object.prototype` property is never read). Borrowed.
pub(crate) struct AnyOptions(AnyValue);

/// An owned value read off [`AnyOptions`]; released when dropped
/// unconverted (a digit option the resolution never coerces).
pub(crate) struct Held(AnyValue);

impl Drop for Held {
    fn drop(&mut self) {
        unsafe { __torajs_anyv_rc_dec(self.0) };
    }
}

impl AnyOptions {
    /// CoerceOptionsToObject: `null` is a TypeError (ToObject); any
    /// other value is read through.
    pub(crate) fn coerce(v: AnyValue) -> Read<AnyOptions> {
        if is_null(v) {
            return Err(Refusal::Type(c"Cannot convert undefined or null to object"));
        }
        Ok(AnyOptions(v))
    }
}

fn pending() -> Read<()> {
    if unsafe { __torajs_throw_check() } != 0 {
        Err(Refusal::Thrown)
    } else {
        Ok(())
    }
}

impl OptionSource for AnyOptions {
    type Value = Held;

    fn get(&mut self, name: &'static str) -> Read<Held> {
        if is_undefined(self.0) {
            return Ok(Held(VALUE_UNDEFINED));
        }
        let v = unsafe {
            let key = __torajs_str_alloc(name.as_ptr(), name.len() as i64);
            let v = crate::member_get_receiver::__torajs_any_member_get_with_receiver(
                self.0,
                key as *const c_void,
                self.0,
            );
            __torajs_str_drop(key as *mut c_void);
            Held(v)
        };
        pending()?;
        Ok(v)
    }

    fn is_undefined(&self, v: &Held) -> bool {
        is_undefined(v.0)
    }

    fn to_string(&mut self, v: Held) -> Read<Vec<u8>> {
        let s = unsafe { __torajs_anyv_to_str(v.0) } as *const u8;
        drop(v);
        if pending().is_err() {
            if !s.is_null() {
                unsafe { __torajs_str_drop(s as *mut c_void) };
            }
            return Err(Refusal::Thrown);
        }
        // a unit above ASCII reads as 0xFF, which no option value or
        // grammar accepts
        let out = unsafe {
            let len = (s.add(STR_LEN_OFF) as *const u32).read() as usize;
            let latin1 = (*(s as *const HeapHeader)).flags & STR_FLAG_IS_LATIN1 != 0;
            let data = s.add(STR_DATA_OFF);
            let out: Vec<u8> = (0..len)
                .map(|i| {
                    let unit = if latin1 {
                        *data.add(i) as u16
                    } else {
                        (data.add(i * 2) as *const u16).read_unaligned()
                    };
                    if unit < 0x80 { unit as u8 } else { 0xFF }
                })
                .collect();
            __torajs_str_drop(s as *mut c_void);
            out
        };
        Ok(out)
    }

    fn to_number(&mut self, v: Held) -> Read<f64> {
        let n = unsafe { __torajs_anyv_to_number(v.0) };
        drop(v);
        pending()?;
        Ok(n)
    }

    fn to_boolean(&mut self, v: &Held) -> bool {
        unsafe { __torajs_anyv_to_bool(v.0) }
    }

    fn is_true(&self, v: &Held) -> bool {
        is_bool(v.0) && as_bool(v.0)
    }
}

/// Record the refusal as a pending throw (a [`Refusal::Thrown`] one
/// already is).
pub(crate) fn raise(r: Refusal) {
    unsafe {
        match r {
            Refusal::Thrown => {}
            Refusal::Range(m) => __torajs_throw_range_error(m.as_ptr()),
            Refusal::Type(m) => __torajs_throw_type_error(m.as_ptr()),
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A plain-data option bag that records every [[Get]] and every
    /// coercion, in order.
    pub(crate) struct Recorder {
        pub(crate) props: Vec<(&'static str, Val)>,
        pub(crate) log: Vec<String>,
    }

    #[derive(Clone, Debug)]
    pub(crate) enum Val {
        Num(f64),
        Str(&'static str),
        Bool(bool),
    }

    impl Recorder {
        pub(crate) fn new(props: &[(&'static str, Val)]) -> Self {
            Recorder {
                props: props.to_vec(),
                log: Vec::new(),
            }
        }

        /// The names read, in order.
        pub(crate) fn gets(&self) -> Vec<&str> {
            self.log
                .iter()
                .filter_map(|l| l.strip_prefix("get "))
                .collect()
        }
    }

    impl OptionSource for Recorder {
        type Value = Option<(&'static str, Val)>;

        fn get(&mut self, name: &'static str) -> Read<Self::Value> {
            self.log.push(format!("get {name}"));
            Ok(self.props.iter().find(|(k, _)| *k == name).cloned())
        }

        fn is_undefined(&self, v: &Self::Value) -> bool {
            v.is_none()
        }

        fn to_string(&mut self, v: Self::Value) -> Read<Vec<u8>> {
            let (name, v) = v.expect("defined");
            self.log.push(format!("string {name}"));
            Ok(match v {
                Val::Num(n) => format!("{n}").into_bytes(),
                Val::Str(s) => s.as_bytes().to_vec(),
                Val::Bool(b) => format!("{b}").into_bytes(),
            })
        }

        fn to_number(&mut self, v: Self::Value) -> Read<f64> {
            let (name, v) = v.expect("defined");
            self.log.push(format!("number {name}"));
            Ok(match v {
                Val::Num(n) => n,
                Val::Str(s) => s.parse().unwrap_or(f64::NAN),
                Val::Bool(b) => b as u8 as f64,
            })
        }

        fn to_boolean(&mut self, v: &Self::Value) -> bool {
            match v.as_ref().expect("defined").1 {
                Val::Num(n) => n != 0.0 && !n.is_nan(),
                Val::Str(s) => !s.is_empty(),
                Val::Bool(b) => b,
            }
        }

        fn is_true(&self, v: &Self::Value) -> bool {
            matches!(v, Some((_, Val::Bool(true))))
        }
    }

    #[test]
    fn get_option_checks_the_listed_values() {
        let mut r = Recorder::new(&[("style", Val::Str("percent")), ("x", Val::Str("bad"))]);
        assert_eq!(
            get_string(&mut r, "style", &["decimal", "percent"]),
            Ok(Some(b"percent".to_vec()))
        );
        assert!(matches!(
            get_string(&mut r, "x", &["a"]),
            Err(Refusal::Range(_))
        ));
        assert_eq!(get_string(&mut r, "absent", &["a"]), Ok(None));
    }

    #[test]
    fn number_option_range_and_floor() {
        let mut r = Recorder::new(&[
            ("a", Val::Num(2.7)),
            ("b", Val::Num(101.0)),
            ("c", Val::Str("x")),
        ]);
        assert_eq!(get_number(&mut r, "a", 0.0, 100.0, None), Ok(Some(2.0)));
        assert!(get_number(&mut r, "b", 0.0, 100.0, None).is_err());
        assert!(get_number(&mut r, "c", 0.0, 100.0, None).is_err());
        assert_eq!(
            get_number(&mut r, "d", 0.0, 100.0, Some(1.0)),
            Ok(Some(1.0))
        );
    }
}
