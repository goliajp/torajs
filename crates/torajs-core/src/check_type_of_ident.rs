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
//! 3. **Built-in namespace** — `console` / `Math` / `Object` /
//!    `Number` / `String` / `Boolean` / `JSON` / `Array` / `Reflect`
//!    / `Date` / `WeakRef` / `WeakMap` / `WeakSet` / `Map` / `Set` /
//!    `Symbol` / `BigInt` / `Promise` / `fs` / `fs_promises` /
//!    `process` / `Bun` → `Type::Object(name)`.
//! 4. **Synthesized intrinsic fns** (`__torajs_*` family) —
//!    `_date_now` / `_date_from_ms` / `_date_from_iso` /
//!    `_date_from_components` / `_proto_register` / `_class_register`
//!    / `_register_native_error` / `_my_class_ref` — known signatures.
//! 5. **Manual GC trigger** — `gc` → `() -> void`.
//! 6. **Distinguished literals** — `undefined` → `Type::Undefined`,
//!    `NaN` / `Infinity` → `Type::Number`.
//! 7. **Unknown** — `Err("unknown identifier ...")`.

use crate::check::{Checker, Type};

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
    match name {
        "console" => Ok(Type::Object("console")),
        // §19.2.1 — the global `eval` as a VALUE (thisArg / arg /
        // identity). Any, like globalThis: the runtime cell rides
        // the any lanes; direct `eval("...")` calls compiled away in
        // the desugar_eval prefix and never reach an ident read.
        "eval" => Ok(Type::Any),
        "Math" => Ok(Type::Object("Math")),
        // RFC 20260807-global-object G2 — `globalThis` as a VALUE is
        // the runtime singleton (an Any-boxed immortal dynobj), so
        // member reads ride the any lanes: unknown names answer
        // undefined (bun parity), known-but-unfilled builtins throw
        // through the member-get miss probe. Static builtin member
        // reads never get here — the G1 desugar rewrote them to bare
        // names. `typeof globalThis` keeps its static "object" lane.
        "globalThis" => Ok(Type::Any),
        "Object" => Ok(Type::Object("Object")),
        "Number" => Ok(Type::Object("Number")),
        "String" => Ok(Type::Object("String")),
        "Boolean" => Ok(Type::Object("Boolean")),
        "JSON" => Ok(Type::Object("JSON")),
        "Array" => Ok(Type::Object("Array")),
        "Reflect" => Ok(Type::Object("Reflect")),
        "Date" => Ok(Type::Object("Date")),
        "WeakRef" => Ok(Type::Object("WeakRef")),
        "WeakMap" => Ok(Type::Object("WeakMap")),
        "WeakSet" => Ok(Type::Object("WeakSet")),
        "Map" => Ok(Type::Object("Map")),
        "Set" => Ok(Type::Object("Set")),
        "Symbol" => Ok(Type::Object("Symbol")),
        "BigInt" => Ok(Type::Object("BigInt")),
        "Promise" => Ok(Type::Object("Promise")),
        // The RegExp constructor as a VALUE. `new RegExp(...)` never
        // came through here (the builtin-new desugar owns it), so the
        // ident stayed unlisted and `RegExp.prototype` answered
        // "unknown identifier" — while the lowerer had carried the
        // ctor-namespace face (proto tag 7, name, length) all along.
        "RegExp" => Ok(Type::Object("RegExp")),
        // RFC 20260711-closure-reflection chunk A — `Function.prototype.<m>`
        // (call/apply/bind method cells) needs the namespace ident typed.
        "Function" => Ok(Type::Object("Function")),
        // RFC 20260730-iterator-global 刀 1 — the `Iterator` global
        // as a VALUE (§27.1.3). The type-annotation head `Iterator<T>`
        // resolves in a separate namespace (check_type_ann collapses
        // it to Any) — recorded boundary, no mechanical conflict.
        "Iterator" => Ok(Type::Object("Iterator")),
        "fs" => Ok(Type::Object("fs")),
        "fs_promises" => Ok(Type::Object("fs_promises")),
        "process" => Ok(Type::Object("process")),
        "Bun" => Ok(Type::Object("Bun")),
        "__torajs_date_now" => Ok(Type::Function(Vec::new(), Box::new(Type::Date))),
        "__torajs_date_from_ms" => Ok(Type::Function(vec![Type::Number], Box::new(Type::Date))),
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
        // RFC 20260730 blade 1 — exotic-subclass factory internals:
        // the zero-arg self-alloc magic (class resolved from the
        // enclosing `__new_<C>` at lower time) and the ctor-side
        // `super(len)` resize.
        "__torajs_arr_subclass_alloc_self"
        | "__torajs_number_wrapper_subclass_alloc_self"
        | "__torajs_string_wrapper_subclass_alloc_self"
        | "__torajs_boolean_wrapper_subclass_alloc_self"
        | "__torajs_function_subclass_alloc_self"
        | "__torajs_map_subclass_alloc_self"
        | "__torajs_set_subclass_alloc_self"
        | "__torajs_promise_subclass_alloc_self"
        | "__torajs_regex_subclass_alloc_self" => {
            Ok(Type::Function(Vec::new(), Box::new(Type::Any)))
        }
        "__torajs_arr_subclass_super_len" => Ok(Type::Function(
            vec![Type::Any, Type::Number],
            Box::new(Type::Any),
        )),
        // Blade 2 — the wrapper `super(v)` kernels coerce any operand
        // (§21.1.1.1 / §22.1.1.1 / §20.3.1.1 all run To* themselves).
        "__torajs_number_wrapper_subclass_super"
        | "__torajs_string_wrapper_subclass_super"
        | "__torajs_boolean_wrapper_subclass_super"
        | "__torajs_promise_subclass_super"
        | "__torajs_regex_subclass_super" => Ok(Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::Any),
        )),
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
