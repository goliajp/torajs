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
//! 4. **Synthesized intrinsic fns** (`__torajs_*` family) —
//!    `_date_now` / `_date_from_ms` / `_date_from_iso` /
//!    `_date_from_components` / `_proto_register` / `_class_register`
//!    / `_register_native_error` / `_my_class_ref` — known signatures.
//! 5. **Manual GC trigger** — `gc` → `() -> void`.
//! 6. **Distinguished literals** — `undefined` → `Type::Undefined`,
//!    `NaN` / `Infinity` → `Type::Number`.
//! 7. **Unknown** — `Err("unknown identifier ...")`.

use crate::check::{Checker, Type};

/// RFC 20260730 blade 1 — exotic-subclass factory internals: the
/// zero-arg self-alloc magics (class resolved from the enclosing
/// `__new_<C>` at lower time), Array's ctor-side `super(len)` resize,
/// and the `super(v)` semantics kernels, which coerce any operand
/// themselves (§21.1.1.1 / §22.1.1.1 / §20.3.1.1 all run To*; the
/// collection / weak-collection twins take the §24.x.1.1 iterable and
/// Date the §21.4.2.1 value ladder). One name family, three shapes —
/// the suffix grammar IS the contract (the heritage table builds
/// every name as `__torajs_<x>_subclass_{alloc_self,super}`), so a
/// per-builtin arm list here re-stated the same fact and grew the
/// registered known-debt cascade below with every new builtin
/// (rotation 373's three pushed it over the fn hard limit).
fn subclass_magic_ty(name: &str) -> Option<Type> {
    let alloc_self = name.strip_prefix("__torajs_").is_some_and(|r| {
        r.strip_suffix("_subclass_alloc_self")
            .is_some_and(|b| !b.is_empty())
    });
    if alloc_self {
        return Some(Type::Function(Vec::new(), Box::new(Type::Any)));
    }
    if name == "__torajs_arr_subclass_super_len" {
        return Some(Type::Function(
            vec![Type::Any, Type::Number],
            Box::new(Type::Any),
        ));
    }
    // The 2+-argument elements form (§23.1.1.3) — the second operand
    // is the ctor's packed rest array, any-admitted.
    if name == "__torajs_arr_subclass_super_elems" {
        return Some(Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::Any),
        ));
    }
    let is_super = name.strip_prefix("__torajs_").is_some_and(|r| {
        r.strip_suffix("_subclass_super")
            .is_some_and(|b| !b.is_empty())
    });
    if is_super {
        return Some(Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::Any),
        ));
    }
    None
}

/// Signatures of the error-family synth intrinsics the class-injection
/// passes write into the AST — `Error`'s prototype install and its
/// §20.5.8.1 `cause` install, the `[[ErrorData]]` / IsConstructor
/// probes, and the own-absence Str sentinel mint. Split out of the
/// cascade because that match is a registered known-debt function and
/// the `cause` install would have grown it.
fn error_synth_ty(name: &str) -> Option<Type> {
    Some(match name {
        "__torajs_error_proto_install" => Type::Function(vec![Type::String], Box::new(Type::Void)),
        "__torajs_error_install_cause" => {
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Void))
        }
        "__torajs_error_is_error" | "__torajs_is_constructor" => {
            Type::Function(vec![Type::Any], Box::new(Type::Boolean))
        }
        "__torajs_undef_str" => Type::Function(Vec::new(), Box::new(Type::String)),
        _ => return None,
    })
}

/// The builtin namespace-object idents — every arm was the identical
/// `"X" => Ok(Type::Object("X"))` shape, collapsed to this table
/// (per-name rationale lives in git history: RegExp / Function /
/// Iterator grew their arms in their own RFCs). `eval` / `globalThis`
/// are NOT here: they type Any (runtime cells riding the any lanes),
/// not namespace objects.
const NS_OBJECT_IDENTS: [&str; 25] = [
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
        "__torajs_date_now" => Ok(Type::Function(Vec::new(), Box::new(Type::Date))),
        "__torajs_date_from_ms" => Ok(Type::Function(vec![Type::Number], Box::new(Type::Date))),
        "__torajs_date_from_value" => Ok(Type::Function(vec![Type::Any], Box::new(Type::Date))),
        "__torajs_date_from_iso" => Ok(Type::Function(vec![Type::String], Box::new(Type::Date))),
        "__torajs_date_from_components" => {
            Ok(Type::Function(vec![Type::Number; 7], Box::new(Type::Date)))
        }
        // RFC 20260708-closure-argv-face — the synthetic
        // `__torajs_arguments` materializer (argv ptr + argc →
        // Array<Any>); lowered in the class-synth lane.
        "__torajs_arguments_materialize" => Ok(Type::Function(
            vec![Type::Any, Type::Number],
            Box::new(Type::Array(Box::new(Type::Any))),
        )),
        // The FLAG_ARR_ARGUMENTS stamp — one call right after the
        // mint (both desugar lanes); lowered in the class-synth lane.
        "__torajs_arguments_mark" => Ok(Type::Function(vec![Type::Any], Box::new(Type::Void))),
        // §10.4.4.6 step 21 — the `arguments.callee` strict read
        // (rewritten by the arguments desugar): runs the
        // %ThrowTypeError% getter at runtime.
        "__torajs_arguments_callee" => Ok(Type::Function(Vec::new(), Box::new(Type::Any))),
        "__torajs_proto_register" => Ok(Type::Function(
            vec![Type::Any, Type::String],
            Box::new(Type::Void),
        )),
        "__torajs_class_register" => Ok(Type::Function(
            vec![Type::Any, Type::String, Type::Number],
            Box::new(Type::Void),
        )),
        "__torajs_static_method_reify" => Ok(Type::Function(
            vec![Type::String, Type::String],
            Box::new(Type::Void),
        )),
        // L3b static-field-reflect (2026-07-22) — third arg is the
        // `__sf_<C>__<f>` global's current value (any field type).
        "__torajs_static_field_reify" => Ok(Type::Function(
            vec![Type::String, Type::String, Type::Any],
            Box::new(Type::Void),
        )),
        "__torajs_class_accessor_reify" | "__torajs_class_static_accessor_reify" => Ok(
            Type::Function(vec![Type::String, Type::String], Box::new(Type::Void)),
        ),
        // RFC 20260802-class-computed-member 刀 2 — the class-decl-
        // position patch for one runtime computed member: (class,
        // sentinel, key expr, kind, is_static).
        "__torajs_class_computed_reify" => Ok(Type::Function(
            vec![
                Type::String,
                Type::String,
                Type::Any,
                Type::Number,
                Type::Number,
            ],
            Box::new(Type::Void),
        )),
        // 刀 3 — the derived-ctor no-super ReferenceError raiser the
        // class desugar appends to super-less derived ctors.
        n if error_synth_ty(n).is_some() => Ok(error_synth_ty(n).unwrap()),
        "__torajs_ctor_no_super_throw" => Ok(Type::Function(Vec::new(), Box::new(Type::Void))),
        n if subclass_magic_ty(n).is_some() => Ok(subclass_magic_ty(n).unwrap()),
        "__torajs_error_stack" => Ok(Type::Function(vec![Type::Any], Box::new(Type::String))),
        "__torajs_register_native_error" => {
            Ok(Type::Function(vec![Type::String], Box::new(Type::Void)))
        }
        "__torajs_my_class_ref" => Ok(Type::Function(vec![Type::String], Box::new(Type::Any))),
        // RFC 20260713 blade 5 cut 4 — generator-proto →
        // %GeneratorPrototype% chain writer (class_globals emits it
        // at module init; lowered in the class-synth lane).
        "__torajs_genfn_chain" => Ok(Type::Function(
            vec![Type::Any, Type::Number],
            Box::new(Type::Void),
        )),
        // RFC 20260730-iterator-global 刀 1 — stripped-heir proto →
        // builtin-proto singleton chain writer (class_globals emits
        // it at module init; lowered in the class-synth lane).
        "__torajs_proto_chain_builtin" => Ok(Type::Function(
            vec![Type::Any, Type::Number],
            Box::new(Type::Void),
        )),
        "gc" => Ok(Type::Function(Vec::new(), Box::new(Type::Void))),
        // RFC 20260810-indirect-argc-abi S3.1 — the S1 hidden-argc
        // param by its synthetic name. `arguments.length` rewrites
        // to this ident on every real-argc face; the `__`-prefix
        // hard-error carve-out below would otherwise reject it.
        "__torajs_argc" => Ok(Type::Number),
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
