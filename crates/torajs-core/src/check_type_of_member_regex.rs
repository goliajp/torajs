//! RegExp instance-method / property arms extracted from
//! [`crate::check_type_of_member::check`]'s top-level
//! `match (&obj_ty, name) { ... }` (chunk 195 — fifth sub-batch
//! of check_type_of_member.rs per-type-family decomposition;
//! mirrors chunks 191-194 try_match shape).
//!
//! Pure type-table — every RegExp arm returns a fixed
//! `Type::Function(args, ret)` literal or a bare scalar
//! (`Type::String` / `Type::Boolean` / `Type::Number`) with no
//! `Checker` / `Ast` state. Phase 1c.1 + ES §22.2.6 surface.
//!
//! Returns `Some(Ok(_))` on hit, `None` when `name` is not a
//! RegExp method / property.

use crate::check::Type;

pub(crate) fn try_match(name: &str) -> Option<Result<Type, String>> {
    let ty = match name {
        // ES §22.2.6.13 — `re.test(s)`. The matching engine
        // in `runtime_regex.c` is the single source of
        // truth for both `re.test(s)` and the `s.match(re)`
        // / `s.replace(re, repl)` paths in v0.2 #1.b/c.
        //
        // RFC 20260716 刀 19 — arg sig relaxed from `Type::String`
        // to `Type::Any` per ES §22.2.6.16 step 3 ToString(str).
        // A StringWrapper / Number / etc. arg routes through
        // `ssa_lower_call_regex_methods::lower_haystack`'s
        // `emit_to_string` coerce (owned Str, dropped after the
        // helper borrow read).
        "test" => Type::Function(vec![Type::Any], Box::new(Type::Boolean)),
        // ES §22.2.6.13 — `re.toString()` returns
        // `/` + source + `/` + flags. Runtime helper
        // `__torajs_regex_to_string` builds the string in
        // one alloc.
        "toString" => Type::Function(Vec::new(), Box::new(Type::String)),
        // T-37 followup — `re.source` returns the original
        // pattern string (no flags, no slashes). Compile-
        // time wires through a runtime intrinsic that
        // wraps re->src_bytes in a Str.
        "source" => Type::String,
        // ES §22.2.6.4 — `re.flags` returns the spec-
        // ordered flag string ("" / "g" / "im" / "gimsuy"
        // / etc.). Order is fixed: g, i, m, s, u, y. The
        // runtime helper `__torajs_regex_get_flags` builds
        // the canonical string.
        "flags" => Type::String,
        // ES §22.2.6.5-10 — boolean flag instance accessors.
        // Each maps to a single bit test on `re.flags`;
        // the runtime helper `__torajs_regex_has_flag(re,
        // flag_bit)` does the AND. ssa_lower emits the
        // appropriate `RE_FLAG_*` byte constant per arm.
        "global" | "ignoreCase" | "multiline" | "dotAll" | "unicode" | "sticky" | "unicodeSets"
        | "hasIndices" => Type::Boolean,
        // P9.4 — `re.lastIndex` is a writable Number per
        // spec §22.2.6.9. ssa_lower routes reads through
        // __torajs_regex_get_last_index; writes through
        // __torajs_regex_set_last_index (see assign-Member
        // arm). Tracks across exec/match when g or y set.
        "lastIndex" => Type::Number,
        // RC-4 F1a — re.exec(s) returns Nullable<Array<Str>>:
        // [matched, group1, group2, ...] on hit, null on miss
        // (spec §22.2.6.2). V3-18 narrowing (`if (m !== null)`)
        // yields the bare Array<Str>; un-narrowed member/index
        // consumption decays with a runtime null guard.
        // RFC 20260716 刀 19 — arg sig relaxed same as `test`
        // above (§22.2.6.4 step 3 ToString(S)).
        "exec" => Type::Function(
            vec![Type::Any],
            Box::new(Type::Nullable(Box::new(Type::Array(Box::new(
                Type::String,
            ))))),
        ),
        // Annex B §B.2.4.1 — `re.compile(pattern?, flags?)`
        // re-initializes the receiver in place and returns it
        // (rotation 447). Both slots take anything: a RegExp donor
        // pattern, ToString-coercible values, or nothing at all
        // (the lowering pads boxed undefined).
        "compile" => Type::Function(vec![Type::Any, Type::Any], Box::new(Type::RegExp)),
        _ => return None,
    };
    Some(Ok(ty))
}
