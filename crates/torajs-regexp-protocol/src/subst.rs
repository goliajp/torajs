//! §22.1.3.19.1 GetSubstitution over code units, for the @@replace
//! algorithm: the captures are whatever the receiver's `exec` result
//! held (ToString'd, or absent), and a `$<name>` reference is a Get on
//! the result's `groups` object — a getter there runs, and its throw
//! is the replacement's completion.

use crate::ops::{Owned, get_key, str_from_units, to_string};
use torajs_anyvalue::nanbox::{AnyValue, is_undefined};
use torajs_anyvalue::regexp_support as av;

/// One capture: its code units, or `None` for undefined.
pub(crate) type Capture = Option<Vec<u16>>;

const DOLLAR: u16 = b'$' as u16;

fn digit(u: u16) -> Option<usize> {
    (b'0' as u16..=b'9' as u16)
        .contains(&u)
        .then(|| (u - b'0' as u16) as usize)
}

/// The expansion of `template` for one match. `None` = a `$<name>`
/// Get or its ToString threw.
pub(crate) unsafe fn get_substitution(
    matched: &[u16],
    s: &[u16],
    position: usize,
    captures: &[Capture],
    named: Option<&Owned>,
    template: &[u16],
) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(template.len());
    let mut i = 0usize;
    while i < template.len() {
        let rest = &template[i..];
        if rest[0] != DOLLAR || rest.len() < 2 {
            out.push(rest[0]);
            i += 1;
            continue;
        }
        let next = rest[1];
        if next == DOLLAR {
            out.push(DOLLAR);
            i += 2;
        } else if next == b'`' as u16 {
            out.extend_from_slice(&s[..position]);
            i += 2;
        } else if next == b'&' as u16 {
            out.extend_from_slice(matched);
            i += 2;
        } else if next == b'\'' as u16 {
            // an exec result may claim a match running past the end
            let tail = (position + matched.len()).min(s.len());
            out.extend_from_slice(&s[tail..]);
            i += 2;
        } else if let Some(d1) = digit(next) {
            let d2 = rest.get(2).copied().and_then(digit);
            let (index, consumed) = match d2 {
                Some(d2) if d1 * 10 + d2 <= captures.len() => (d1 * 10 + d2, 3),
                _ => (d1, 2),
            };
            if (1..=captures.len()).contains(&index) {
                if let Some(c) = &captures[index - 1] {
                    out.extend_from_slice(c);
                }
            } else {
                out.extend_from_slice(&rest[..consumed]);
            }
            i += consumed;
        } else if next == b'<' as u16 {
            let gt = rest.iter().position(|&u| u == b'>' as u16);
            match (gt, named) {
                (Some(gt), Some(groups)) => {
                    let name = unsafe { str_from_units(&rest[2..gt]) };
                    let capture = unsafe { get_key(groups.get(), name.ptr()) }?;
                    if !is_undefined(capture.get()) {
                        out.extend(unsafe { to_string(capture.get()) }?.units());
                    }
                    i += gt + 1;
                }
                _ => {
                    out.extend_from_slice(&rest[..2]);
                    i += 2;
                }
            }
        } else {
            out.push(DOLLAR);
            i += 1;
        }
    }
    Some(out)
}

/// `namedCaptures` for the substitution: undefined stays absent,
/// anything else is ? ToObject'd.
pub(crate) unsafe fn named_object(groups: AnyValue) -> Option<Option<Owned>> {
    if is_undefined(groups) {
        return Some(None);
    }
    // ToObject: null is the one remaining value it refuses
    if torajs_anyvalue::nanbox::is_null(groups) {
        unsafe { crate::ops::type_error(c"cannot convert null to object") };
        return None;
    }
    let obj = Owned::adopt(unsafe { av::__torajs_any_to_object(groups) });
    if crate::ops::throw_pending() {
        return None;
    }
    Some(Some(obj))
}
