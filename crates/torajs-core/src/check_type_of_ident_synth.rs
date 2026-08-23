//! The compiler-synthesized ident family of
//! [`crate::check_type_of_ident`] — step 4 of that file's resolution
//! order, in its own sibling so the host's `check` stays under the
//! function cap.
//!
//! Every name here is one a desugar or a lowering pass writes, and
//! its type is the signature the intrinsic it lowers to declares.
//! `gc` rides along as the one user-spellable name in the group: it
//! is registered into the SAME intrinsic `fn_table` beside them
//! (`ssa_lower_intrinsics_init_c` aliases it to `cycle_collect`), so
//! its answer belongs with theirs rather than with the distinguished
//! literals.
//!
//! `None` means "not one of ours" and the host falls through to its
//! own arms. A real binding of any of these names has already won
//! before this is reached.

use crate::check::Type;

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
    // Buffer-family blade — the shared TypedArray pair breaks both
    // suffix-grammar shapes (its mint takes the kind discriminant,
    // its super three value slots), so it resolves ahead of them.
    if name == "__torajs_typedarray_subclass_alloc_self" {
        return Some(Type::Function(vec![Type::Number], Box::new(Type::Any)));
    }
    if name == "__torajs_typedarray_subclass_super" {
        return Some(Type::Function(
            vec![Type::Any, Type::Any, Type::Any, Type::Any],
            Box::new(Type::Any),
        ));
    }
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
    // The 2+-argument elements form (§23.1.1.3) and Date's components
    // form (§21.4.2.1 step 6) — the second operand is the ctor's
    // packed rest array, any-admitted.
    if matches!(
        name,
        "__torajs_arr_subclass_super_elems" | "__torajs_date_subclass_super_components"
    ) {
        return Some(Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::Any),
        ));
    }
    // RegExp's §22.2.4.1 two-argument form — `(this, pattern, flags)`,
    // all any-admitted.
    if name == "__torajs_regex_subclass_super_flags" {
        return Some(Type::Function(
            vec![Type::Any, Type::Any, Type::Any],
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

pub(crate) fn try_type(name: &str) -> Option<Result<Type, String>> {
    let t = match name {
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
        // §13.3.6 call off a Super Reference: base, key,
        // receiver, args pack — every slot any-world, and the
        // product is whatever the method answered.
        "__torajs_super_prop_get" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::Any],
            Box::new(Type::Any),
        )),
        "__torajs_super_prop_set" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::Any, Type::Any],
            Box::new(Type::Any),
        )),
        "__torajs_super_prop_call" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::Any, Type::Any],
            Box::new(Type::Any),
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
        // 419-01 — the FIELD lane's ToPropertyKey shell: the key
        // evaluated at the class-decl position, answered as the `any`
        // the `__ccmk_<C>_<n>` global holds.
        "__torajs_class_computed_key" => Ok(Type::Function(vec![Type::Any], Box::new(Type::Any))),
        // RFC 20260820-dstr-deferred-close — the deferred
        // IteratorClose a suspendable destructuring pattern's finally
        // calls on its parked iterator slot.
        "__torajs_dstr_close_pending" => Ok(Type::Function(vec![Type::Any], Box::new(Type::Void))),
        // 刀 D — the rest element's post-suspension drain: (parked
        // resume value, raw source) → the tail as an `any` array.
        "__torajs_dstr_drain_rest" => Ok(Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::Any),
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
        // RFC 20260820-ctor-return-override — §10.2.2 step 13 pick and
        // the own-element carry beside it. The pick answers `any`
        // because that is the honest type: what a constructor hands
        // back may be any object at all.
        "__torajs_ctor_ret_value" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::Boolean],
            Box::new(Type::Any),
        )),
        "__torajs_ctor_ret_carry" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::String],
            Box::new(Type::Void),
        )),
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
        _ => return None,
    };
    Some(t)
}
