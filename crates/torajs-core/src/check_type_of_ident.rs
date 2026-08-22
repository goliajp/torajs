//! `Expr::Ident(name)` typecheck pulled out of
//! [`crate::check::Checker::type_of_inner`]'s `Expr::Ident` arm as
//! chunk-90 of the type_of_inner decomp.
//!
//! Resolution order:
//!
//! 1. **Local binding** (`Checker::lookup`) — TS-shape: reads of an
//!    aliased / moved binding succeed (both `s` and `n` after
//!    `let n = s` reference the same heap). Errors fire at transfer
//!    sites only (see `consume`).
//! 2. **Module global** (`Checker::globals`) — typed at declaration
//!    site.
//! 3. **Built-in namespace** — the [`NS_OBJECT_IDENTS`] table →
//!    `Type::Object(name)`.
//! 4. **Synthesized intrinsic fns** (`__torajs_*` family, plus the
//!    `gc` alias registered beside them) — the
//!    [`crate::check_type_of_ident_synth`] sibling; each name answers
//!    the signature the intrinsic it lowers to declares.
//! 5. **Distinguished literals** — `undefined` → `Type::Undefined`,
//!    `NaN` / `Infinity` → `Type::Number`.
//! 6. **Unknown** — `Err("unknown identifier ...")`.

use crate::check::{Checker, Type};

/// The builtin namespace-object idents — every arm was the identical
/// `"X" => Ok(Type::Object("X"))` shape, collapsed to this table
/// (per-name rationale lives in git history: RegExp / Function /
/// Iterator grew their arms in their own RFCs). `eval` / `globalThis`
/// are NOT here: they type Any (runtime cells riding the any lanes),
/// not namespace objects.
const NS_OBJECT_IDENTS: [&str; 27] = [
    "console",
    "Math",
    "Object",
    "Number",
    "String",
    "Boolean",
    "JSON",
    "Array",
    "Reflect",
    "Date",
    "WeakRef",
    "WeakMap",
    "WeakSet",
    "Map",
    "Set",
    "Symbol",
    "BigInt",
    "Promise",
    "RegExp",
    "Function",
    "Iterator",
    "Proxy",
    "ArrayBuffer",
    "fs",
    "fs_promises",
    "process",
    "Bun",
];

pub(crate) fn check(
    checker: &mut Checker,
    eid: crate::ast::ExprId,
    name: &str,
) -> Result<Type, String> {
    if let Some(info) = checker.lookup(name) {
        // A read marked undeclared by an earlier speculative pass
        // (hoist pre-typing under an incomplete scope) that NOW
        // resolves is a legal forward reference — self-heal the mark.
        checker.undeclared_reads.remove(&eid);
        return Ok(info.ty);
    }
    if let Some(ty) = checker.globals.get(name) {
        let ty = ty.clone();
        checker.undeclared_reads.remove(&eid);
        return Ok(ty);
    }
    if let Some(&ns) = NS_OBJECT_IDENTS.iter().find(|&&n| n == name) {
        return Ok(Type::Object(ns));
    }
    // The compiler-synthesized name family (`__torajs_*`, plus the
    // one user-spellable alias registered beside them) lives in the
    // `_synth` sibling. It sits AFTER the three resolution steps
    // above — a real binding of that name still wins — and before
    // the literal arms below, which it shares no name with.
    if let Some(r) = crate::check_type_of_ident_synth::try_type(name) {
        return r;
    }
    match name {
        // §19.2.1 — the global `eval` as a VALUE (thisArg / arg /
        // identity). Any, like globalThis: the runtime cell rides
        // the any lanes; direct `eval("...")` calls compiled away in
        // the desugar_eval prefix and never reach an ident read.
        "eval" => Ok(Type::Any),
        // RFC 20260807-global-object G2 — `globalThis` as a VALUE is
        // the runtime singleton (an Any-boxed immortal dynobj), so
        // member reads ride the any lanes: unknown names answer
        // undefined (bun parity), known-but-unfilled builtins throw
        // through the member-get miss probe. Static builtin member
        // reads never get here — the G1 desugar rewrote them to bare
        // names. `typeof globalThis` keeps its static "object" lane.
        "globalThis" => Ok(Type::Any),
        // §19.2.2-6 function properties of the global object as bare
        // VALUES — the ns-static cells shipped (rotation 368), so a
        // value-position read answers the concrete fn type exactly
        // like the `Number.parseInt` member read does. Direct calls
        // never get here (the bare_globals early route fires first).
        "parseInt" => Ok(Type::Function(
            vec![Type::String, Type::Number],
            Box::new(Type::Number),
        )),
        "parseFloat" => Ok(Type::Function(vec![Type::String], Box::new(Type::Number))),
        "isNaN" | "isFinite" => Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean))),
        "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent" => {
            Ok(Type::Function(vec![Type::String], Box::new(Type::String)))
        }
        "undefined" => Ok(Type::Undefined),
        "NaN" | "Infinity" => Ok(Type::Number),
        // The undefined an async body's fall-through tail settles with
        // (`desugar_async`). It stands where a value of the declared
        // inner type is expected, and the SSA lowering gives it that
        // width's undefined sentinel, so it types as the annotation it
        // carries rather than as `Type::Undefined`.
        other if other.starts_with(crate::ast::UNDEF_SLOT_MARKER) => {
            let ann = &other[crate::ast::UNDEF_SLOT_MARKER.len()..];
            crate::check_type_ann::resolve_type_ann_full(
                ann,
                &checker.aliases,
                &[],
                &checker.generic_alias_decls,
            )
            .ok_or_else(|| format!("unresolvable async tail type `{ann}`"))
        }
        // RFC 20260730-undeclared-ident (§6.2.5.5) — an expression-
        // position read that resolves nowhere is not a compile
        // reject: it types `Any`, gets marked (surfaced as one
        // deduped end-of-pipeline warning), and raises a catchable
        // ReferenceError when evaluated (see
        // ssa_lower_ident::try_undeclared_read_throw). Two carve-outs
        // stay hard errors: `__`-prefixed names are compiler-
        // synthesized (an unresolved one is a compiler bug, not user
        // code), and known builtin globals (`parseInt` /
        // `queueMicrotask` / …) that exist only as NAME-keyed call
        // lanes — a speculative wedge probe types their callee
        // Ident, and a mark there turns every such call into a bogus
        // runtime throw (gate caught 28: parseInt ×10,
        // queueMicrotask ×5, isNaN, …). A speculative pre-pass mark
        // on a name that later resolves self-heals at the resolution
        // sites above.
        other => {
            if other.starts_with("__") || crate::check::is_known_builtin_global(other) {
                return Err(format!("unknown identifier `{other}`"));
            }
            checker.undeclared_reads.insert(eid, other.to_string());
            Ok(Type::Any)
        }
    }
}
