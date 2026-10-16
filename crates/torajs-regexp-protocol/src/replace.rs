//! §22.2.6.11 `RegExp.prototype[@@replace]`.
//!
//! The results are collected first (steps 11-12) and only then read
//! (step 14) — a getter on a result runs after every `exec` call, and
//! each result's properties are read in the order the spec lists them:
//! `length`, `0`, `index`, the captures, `groups`.

use crate::ops::{
    ABRUPT, Owned, OwnedStr, advance_string_index, box_number, flags_of, get, get_index, has_flag,
    last_index_length, regexp_exec, set_last_index, str_from_units, to_integer_or_infinity,
    to_length, to_string,
};
use crate::subst::{Capture, get_substitution, named_object};
use torajs_anyvalue::nanbox::{AnyValue, VALUE_UNDEFINED, box_int32, is_null, is_undefined};
use torajs_anyvalue::regexp_support as av;

/// §22.2.6.11 steps 3-16.
pub(crate) unsafe fn symbol_replace(
    rx: AnyValue,
    string: AnyValue,
    replace_value: AnyValue,
) -> AnyValue {
    unsafe {
        let Some(s) = to_string(string) else {
            return ABRUPT;
        };
        let functional = av::is_callable(replace_value);
        let template = if functional {
            None
        } else {
            let Some(t) = to_string(replace_value) else {
                return ABRUPT;
            };
            Some(t.units())
        };
        let Some(flags) = flags_of(rx) else {
            return ABRUPT;
        };
        let global = has_flag(&flags, b'g');
        let units = s.units();
        let Some(results) = collect_results(rx, &s, &units, global, &flags) else {
            return ABRUPT;
        };
        let mut acc: Vec<u16> = Vec::new();
        let mut next_source_position = 0usize;
        for result in &results {
            let r = result.get();
            let Some(m) = read_match(r, units.len()) else {
                return ABRUPT;
            };
            let replacement = if functional {
                call_replacer(replace_value, &m, &s)
            } else {
                substitute(&m, &units, template.as_deref().unwrap_or(&[]))
            };
            let Some(replacement) = replacement else {
                return ABRUPT;
            };
            if m.position >= next_source_position {
                acc.extend_from_slice(&units[next_source_position..m.position]);
                acc.extend_from_slice(&replacement);
                next_source_position = m.position + m.matched.len();
            }
        }
        if next_source_position < units.len() {
            acc.extend_from_slice(&units[next_source_position..]);
        }
        str_from_units(&acc).take_boxed()
    }
}

/// Steps 9-12: the `exec` results, all of them before any is read.
unsafe fn collect_results(
    rx: AnyValue,
    s: &OwnedStr,
    units: &[u16],
    global: bool,
    flags: &[u16],
) -> Option<Vec<Owned>> {
    unsafe {
        let full_unicode = has_flag(flags, b'u') || has_flag(flags, b'v');
        if global {
            set_last_index(rx, box_int32(0))?;
        }
        let mut results = Vec::new();
        loop {
            let result = regexp_exec(rx, s)?;
            if is_null(result.get()) {
                break;
            }
            results.push(result);
            if !global {
                break;
            }
            let last = results.last().map_or(VALUE_UNDEFINED, Owned::get);
            let m0 = get_index(last, 0)?;
            if to_string(m0.get())?.is_empty() {
                let this_index = last_index_length(rx)?;
                let next = advance_string_index(units, this_index, full_unicode);
                set_last_index(rx, box_number(next as f64))?;
            }
        }
        Some(results)
    }
}

/// What step 14 reads off one result.
struct MatchRecord {
    matched: Vec<u16>,
    matched_str: OwnedStr,
    position: usize,
    captures: Vec<Capture>,
    capture_values: Vec<Owned>,
    groups: Owned,
}

/// Steps 14.a-14.j.
unsafe fn read_match(r: AnyValue, len_s: usize) -> Option<MatchRecord> {
    unsafe {
        let length = get(r, b"length")?;
        let result_length = to_length(length.get())?;
        let n_captures = (result_length - 1).max(0);
        let m0 = get_index(r, 0)?;
        let matched_str = to_string(m0.get())?;
        let index = get(r, b"index")?;
        // max / min, not clamp: clamp's NaN panic formats an f64 and
        // links the whole float printer into every program
        let position = to_integer_or_infinity(index.get())?
            .max(0.0)
            .min(len_s as f64) as usize;
        let mut captures = Vec::new();
        let mut capture_values = Vec::new();
        let mut n = 1;
        while n <= n_captures {
            let cap = get_index(r, n)?;
            if is_undefined(cap.get()) {
                captures.push(None);
                capture_values.push(cap);
            } else {
                let c = to_string(cap.get())?;
                captures.push(Some(c.units()));
                capture_values.push(Owned::adopt(c.take_boxed()));
            }
            n += 1;
        }
        let groups = get(r, b"groups")?;
        Some(MatchRecord {
            matched: matched_str.units(),
            matched_str,
            position,
            captures,
            capture_values,
            groups,
        })
    }
}

/// Step 14.k: Call(replaceValue, undefined, «matched, …captures,
/// position, S[, groups]») and ToString of its answer.
unsafe fn call_replacer(f: AnyValue, m: &MatchRecord, s: &OwnedStr) -> Option<Vec<u16>> {
    unsafe {
        let mut args = Vec::with_capacity(m.capture_values.len() + 4);
        args.push(m.matched_str.boxed());
        args.extend(m.capture_values.iter().map(Owned::get));
        args.push(box_number(m.position as f64));
        args.push(s.boxed());
        if !is_undefined(m.groups.get()) {
            args.push(m.groups.get());
        }
        let v = Owned::adopt(av::__torajs_any_call_with_this(
            f,
            VALUE_UNDEFINED,
            args.as_ptr(),
            args.len() as i64,
        ));
        if crate::ops::throw_pending() {
            return None;
        }
        Some(to_string(v.get())?.units())
    }
}

/// Step 14.l: GetSubstitution over the template.
unsafe fn substitute(m: &MatchRecord, s: &[u16], template: &[u16]) -> Option<Vec<u16>> {
    unsafe {
        let named = named_object(m.groups.get())?;
        get_substitution(
            &m.matched,
            s,
            m.position,
            &m.captures,
            named.as_ref(),
            template,
        )
    }
}
