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
pub(crate) const NS_OBJECT_IDENTS: [&str; 42] = [
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
    "DataView",
    "FinalizationRegistry",
    // RFC 20260823-typedarray-substrate 刀 2 — the eleven §23.2
    // constructors, each also a namespace for its own statics.
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
    "Float16Array",
    "fs",
    "fs_promises",
    "process",
    "Bun",
    "performance",
];

pub(crate) fn check(
    checker: &mut Checker,
    ast: &crate::ast::Ast,
    eid: crate::ast::ExprId,
    name: &str,
) -> Result<Type, String> {
    // §16.2.1.6 — a read the module resolver judged unresolvable in
    // its own module (`Ast::module_foreign_reads`) is an unresolvable
    // Reference whatever the flat top level would answer. The mark
    // names another module's user declaration, so the synthesized-
    // name and builtin carve-outs below have nothing to say about it.
    if ast.module_foreign_reads.contains(&eid) {
        checker.undeclared_reads.insert(eid, name.to_string());
        return Ok(Type::Any);
    }
    if let Some(info) = checker.lookup(name) {
        // A read marked undeclared by an earlier speculative pass
        // (hoist pre-typing under an incomplete scope) that NOW
        // resolves is a legal forward reference — self-heal the mark.
        checker.undeclared_reads.remove(&eid);
        if let Some(lit) = info.union.literal {
            return Ok(lit);
        }
        if info.ty == Type::Any {
            note_scalar(checker, eid, info.held_decl.clone());
        }
        return Ok(held_face(checker, ast, eid, info.held_any, info.ty));
    }
    if let Some(ty) = checker.globals.get(name) {
        let ty = ty.clone();
        checker.undeclared_reads.remove(&eid);
        let held = checker.held_any.globals.contains(name);
        let scalar = checker.held_any.scalar_globals.get(name).cloned();
        note_scalar(checker, eid, scalar);
        return Ok(held_face(checker, ast, eid, held, ty));
    }
    // §8.1.1.1 — a read ahead of its top-level declaration reaches that
    // binding, uninitialized; the lowering raises the ReferenceError.
    // A builtin of the same name below is not what it reads.
    if checker.tdz_reads.contains(&eid) {
        return Ok(Type::Any);
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
        // through the member-get miss probe. A static builtin member
        // read is here only where the G1 desugar kept the member form
        // (`globalthis_member`); the rest became bare names. `typeof
        // globalThis` keeps its static "object" lane.
        "globalThis" => Ok(Type::Any),
        // §19.2.2-6 function properties of the global object as bare
        // VALUES — the ns-static cells shipped (rotation 368), so a
        // value-position read answers the concrete fn type exactly
        // like the `Number.parseInt` member read does. Direct calls
        // never get here (the bare_globals early route fires first).
        // …unless the module writes that property of the global
        // object. Then the fold — cell for the value, dedicated route
        // for the call — is reading something the write never
        // reached, and the read has to go to the object itself.
        _ if checker.proto_shadow.writes_global(name) => Ok(Type::Any),
        "parseInt" => Ok(Type::Function(
            vec![Type::String, Type::Number],
            Box::new(Type::Number),
        )),
        "parseFloat" => Ok(Type::Function(vec![Type::String], Box::new(Type::Number))),
        "isNaN" | "isFinite" => Ok(Type::Function(vec![Type::Any], Box::new(Type::Boolean))),
        "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent"
        // §B.2.1 escape / unescape — same shape, String in, String out.
        | "escape" | "unescape" => {
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
        // stay hard errors: a name TR MINTED (an unresolved one is a
        // compiler bug, not user code — and the judge is provenance,
        // `is_synthesized_dunder`, because the `__` prefix alone calls
        // sputnik's `__ref` / `__key` user identifiers compiler names),
        // and known builtin globals (`parseInt` /
        // `queueMicrotask` / …) that exist only as NAME-keyed call
        // lanes — a speculative wedge probe types their callee
        // Ident, and a mark there turns every such call into a bogus
        // runtime throw (gate caught 28: parseInt ×10,
        // queueMicrotask ×5, isNaN, …). A speculative pre-pass mark
        // on a name that later resolves self-heals at the resolution
        // sites above.
        other => {
            // r505 (A12) — the class object / prototype cells are
            // fn-locals of the synthesized prologue (`__cprologue`),
            // so a read from main or a method body no longer resolves
            // lexically; it is the registry read the lowering already
            // routes every non-local read through (`class_get` /
            // `proto_get`), and its type is the cell's: any. Only for
            // a class the program declares — a user ident that merely
            // starts with the prefix still fails loud below.
            if let Some(c) = other
                .strip_prefix("__class_")
                .or_else(|| other.strip_prefix("__proto_"))
                && checker.class_names.contains(c)
            {
                if ast.type_only_classes.contains(c) {
                    return Err(crate::check_type_of_misc::type_only_value_error(c));
                }
                return Ok(Type::Any);
            }
            if checker.is_synthesized_dunder(other) || crate::check::is_known_builtin_global(other)
            {
                return Err(format!("unknown identifier `{other}`"));
            }
            // §8.1.1.1 — a dead-zone read is not an unresolvable
            // Reference: the binding exists, it is just uninitialized,
            // and the lowerer raises its own message there. Marking it
            // undeclared here would add a second raise plus the
            // end-of-pipeline `unknown identifier` warning for a name
            // the program does declare.
            // TS2693 / TS1361 — a name that declares only a type (an
            // alias, an interface, or one a type-only import brought
            // in) has no value to read; TypeScript rejects the program.
            if (checker.aliases.contains_key(other)
                || checker.generic_alias_decls.contains_key(other))
                && !checker.class_names.contains(other)
            {
                return Err(format!(
                    "`{other}` only refers to a type, but is being used as a value here"
                ));
            }
            if !checker.tdz_reads.contains(&eid) {
                checker.undeclared_reads.insert(eid, other.to_string());
            }
            Ok(Type::Any)
        }
    }
}

/// A read of a binding that holds an `any` value types as `any` unless
/// a member access reads through it (`crate::check_stmt_let_decl::held_any`);
/// its declared type is kept for the TypeScript rules. A receiver
/// declared with a class or a function type types `any` too: the
/// class's members resolve to its own methods and offsets, a function's
/// to the closure's, and §13.3.2 / §13.3.6 read them off the value the
/// binding holds, which need not be an instance of the class or a
/// function.
fn held_face(
    checker: &mut Checker,
    ast: &crate::ast::Ast,
    eid: crate::ast::ExprId,
    held: bool,
    ty: Type,
) -> Type {
    let by_name = match &ty {
        Type::Nullable(t) => matches!(**t, Type::ClassRef(_) | Type::Function(..)),
        t => matches!(t, Type::ClassRef(_) | Type::Function(..)),
    };
    if held && (by_name || !checker.held_any.is_receiver(ast, eid)) {
        checker.held_any.reads.insert(eid, ty);
        Type::Any
    } else {
        ty
    }
}

/// A read of a `number` / `string` / `boolean` binding that holds an
/// `any` value (`crate::check_stmt_let_decl::any_scalar_slot`): typed
/// `any`, its declared type kept for the TypeScript rules.
fn note_scalar(checker: &mut Checker, eid: crate::ast::ExprId, declared: Option<Type>) {
    if let Some(t) = declared {
        checker.held_any.reads.insert(eid, t);
    }
}
