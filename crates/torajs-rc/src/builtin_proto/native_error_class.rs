//! §20.5 Error-family class registry — the injected `Error` /
//! NativeError CLASS objects, keyed by a fixed family index, so the
//! globalThis fill (torajs-anyvalue) can answer the SAME identity the
//! bare name reads (`globalThis.TypeError === TypeError`).
//!
//! The recorder is torajs-meta's `__torajs_error_proto_install` — the
//! one runtime site that sees both the class NAME and its registered
//! tag; it reaches the store through the `_record` C symbol (meta
//! keeps a zero-Cargo-dep tree). A program that never injects a
//! family member leaves its slot 0 and the dynamic read keeps the
//! fill's loud MISSING_KNOWN posture. The classes live in
//! module-scope let bindings (process lifetime), so the slot holds a
//! borrowed immediate like `CLASSES_BY_TAG_IMM` does.

use core::sync::atomic::{AtomicU64, Ordering};

/// Fixed family order — the index is ABI between the recorder
/// (torajs-meta `classmeta/error_family.rs` mirrors this list)
/// and the reader (torajs-anyvalue globalThis fill). Append-only.
pub const NATIVE_ERROR_FAMILY: [&str; 9] = [
    "Error",
    "TypeError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "EvalError",
    "URIError",
    "AggregateError",
    "SuppressedError",
];

static CLASS_ANYVS: [AtomicU64; NATIVE_ERROR_FAMILY.len()] =
    [const { AtomicU64::new(0) }; NATIVE_ERROR_FAMILY.len()];

/// Record the class-object AnyValue immediate under family index
/// `idx`. Out-of-range indexes are ignored (defensive — the
/// recorder derives `idx` from its own mirror of the family
/// list).
#[unsafe(no_mangle)]
pub extern "C" fn __torajs_native_error_class_record(idx: i64, class_anyv: u64) {
    if idx < 0 || idx as usize >= CLASS_ANYVS.len() {
        return;
    }
    CLASS_ANYVS[idx as usize].store(class_anyv, Ordering::Release);
}

/// The registered class-object AnyValue for family index `idx`,
/// 0 when this program never injected that class.
pub fn native_error_class(idx: usize) -> u64 {
    CLASS_ANYVS
        .get(idx)
        .map_or(0, |slot| slot.load(Ordering::Acquire))
}
