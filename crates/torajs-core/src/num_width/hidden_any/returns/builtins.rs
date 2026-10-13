//! What a value of a union of builtin types answers, as the return walk
//! of `hidden_any::returns` reads it.
//!
//! The checker types a `string | number` or an `any[] | string` `any`,
//! while the library still fixes what each of its members answers:
//! `x.toString()` a string, `x.length` a number, `x[0]` a string of a
//! string and an element of an array (§22.1.3, §23.1.3). A return
//! declared `number` converts a string, so it seeds when an arm answers
//! a value of another kind; an element of an `any[]` may be of any kind.

use super::kinds::{Kind, Kinds};
use crate::check::Type;

/// One arm of a union of builtin types.
#[derive(Clone, Copy, Debug)]
pub(super) enum Arm {
    /// A scalar of this kind (a bigint or a symbol is `Kind::Other`).
    Scalar(Kind),
    /// An array whose elements are of these kinds.
    Array(Kinds),
}

impl Arm {
    /// The kinds a value of this arm is.
    pub(super) fn kinds(self) -> Kinds {
        match self {
            Arm::Scalar(k) => Kinds::one(k),
            Arm::Array(_) => Kinds::one(Kind::Other),
        }
    }
}

/// The kind of a scalar annotation arm: a builtin type or a literal type.
pub(super) fn scalar_kind(arm: &str) -> Option<Kind> {
    Some(match arm {
        "string" => Kind::String,
        "number" => Kind::Number,
        "boolean" | "true" | "false" => Kind::Boolean,
        "bigint" | "symbol" => Kind::Other,
        _ if arm.starts_with(['"', '\'', '`']) => Kind::String,
        // `1n` and `-1n` are bigints
        _ if arm.starts_with(|c: char| c == '-' || c.is_ascii_digit()) => {
            match arm.ends_with('n') {
                true => Kind::Other,
                false => Kind::Number,
            }
        }
        _ => return None,
    })
}

/// The arms of a union whose members the checker keeps
/// (`check::union_narrow`), narrowed where it is read, when every
/// member is a builtin type.
pub(super) fn arms_of(members: &[Type]) -> Option<Vec<Arm>> {
    let mut arms = Vec::new();
    for m in members {
        arms.push(match m {
            // the any lane a narrowed union travels in (a union holding
            // `any` is `any`), and the nullish members
            Type::Any | Type::Null | Type::Undefined | Type::Void => continue,
            Type::Array(el) => Arm::Array(match &**el {
                Type::Any => Kinds::ANY,
                t => Kinds::one(Kind::of(t)),
            }),
            Type::Literal(l) => Arm::Scalar(Kind::of(&l.base())),
            Type::BigInt => Arm::Scalar(Kind::Other),
            t => match Kind::of(t) {
                Kind::Other => return None,
                k => Arm::Scalar(k),
            },
        });
    }
    (!arms.is_empty()).then_some(arms)
}

/// The kinds what a value of one of `arms` answers: its member `name`
/// read, or called when `call`. Calling a method an arm lacks throws,
/// and TS reads a member off a union only where a guard leaves the
/// arms that declare it, so only the arms the walk knows the member of
/// answer; [`Kinds::ANY`] when it knows it of none (a field the program
/// added to an array).
pub(super) fn member(arms: &[Arm], name: &str, call: bool) -> Kinds {
    let known = arms.iter().filter_map(|arm| arm_member(*arm, name, call));
    known.reduce(Kinds::with).unwrap_or(Kinds::ANY)
}

/// The kinds a value of one of `arms` is.
pub(super) fn values(arms: &[Arm]) -> Kinds {
    (arms.iter()).fold(Kinds::NONE, |ks, arm| ks.with(arm.kinds()))
}

/// The kinds an element read `x[i]` of a value of one of `arms`
/// answers, by a numeric key.
pub(super) fn element(arms: &[Arm]) -> Kinds {
    (arms.iter()).fold(Kinds::NONE, |ks, arm| {
        ks.with(match arm {
            Arm::Scalar(Kind::String) => Kinds::one(Kind::String),
            Arm::Scalar(_) => Kinds::one(Kind::Other),
            Arm::Array(el) => *el,
        })
    })
}

fn arm_member(arm: Arm, m: &str, call: bool) -> Option<Kinds> {
    use Kind::*;
    let k = match (arm, call) {
        (Arm::Scalar(String) | Arm::Array(_), false) if m == "length" => Number,
        // a method read as a value; a field the library does not
        // declare is one the program added, or a read TS refuses
        // unless a guard leaves another arm
        (_, false) => return arm_member(arm, m, true).map(|_| Kinds::one(Other)),
        (Arm::Array(el), true) => match m {
            "at" | "pop" | "shift" | "find" | "findLast" => return Some(el),
            "toString" | "toLocaleString" | "join" => String,
            "indexOf" | "lastIndexOf" | "findIndex" | "findLastIndex" | "push" | "unshift" => {
                Number
            }
            "includes" | "some" | "every" => Boolean,
            "map" | "filter" | "slice" | "concat" | "splice" | "sort" | "reverse" | "flat"
            | "flatMap" | "fill" | "copyWithin" | "keys" | "values" | "entries" | "forEach"
            | "toSorted" | "toReversed" | "toSpliced" | "with" => Other,
            _ => return None,
        },
        (Arm::Scalar(String), true) => match m {
            "toString" | "valueOf" | "toLocaleString" | "at" | "charAt" | "slice" | "substring"
            | "substr" | "toUpperCase" | "toLowerCase" | "toLocaleUpperCase"
            | "toLocaleLowerCase" | "trim" | "trimStart" | "trimEnd" | "padStart" | "padEnd"
            | "repeat" | "concat" | "replace" | "replaceAll" | "normalize" | "toWellFormed" => {
                String
            }
            "indexOf" | "lastIndexOf" | "charCodeAt" | "codePointAt" | "localeCompare"
            | "search" => Number,
            "includes" | "startsWith" | "endsWith" | "isWellFormed" => Boolean,
            "split" | "match" | "matchAll" => Other,
            _ => return None,
        },
        (Arm::Scalar(k), true) => match m {
            "toString" | "toLocaleString" => String,
            "toFixed" | "toPrecision" | "toExponential" if k == Number => String,
            "valueOf" => k,
            _ => return None,
        },
    };
    Some(Kinds::one(k))
}
