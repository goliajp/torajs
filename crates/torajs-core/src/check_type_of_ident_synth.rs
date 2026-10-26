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

use crate::ast::class_unit::{
    UNIT_HOME, UNIT_STATIC_ACCESSOR_REIFY, UNIT_STATIC_FIELD_DEFINE, UNIT_STATIC_METHOD_REIFY,
    UNIT_WIRE,
};
use crate::ast::private_brand::FIELD_DEFINE;
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
/// §20.5.8.1 `cause` install, the `[[ErrorData]]` / IsConstructor /
/// SpeciesConstructor probes, and the own-absence Str sentinel mint.
/// Split out of the
/// cascade because that match is a registered known-debt function and
/// the `cause` install would have grown it.
/// RFC 20260820-dstr-deferred-close — the destructuring-pattern
/// family, kept together the way `error_synth_ty` keeps the error
/// one: every member takes and answers `any`, and what distinguishes
/// them is which half of §13.15.5 they serve.
fn dstr_synth_ty(name: &str) -> Option<Type> {
    Some(match name {
        // The deferred IteratorClose a suspendable pattern's finally
        // calls on its parked iterator slot.
        "__torajs_dstr_close_pending" => Type::Function(vec![Type::Any], Box::new(Type::Void)),
        // 刀 D — the rest element's post-suspension drain: (parked
        // resume value, raw source) → the tail as an `any` array.
        // §13.15.5.5 per-element step has the same shape: (park slot
        // read, raw source) → that element's value. The park is an
        // argument by NAME there — the lowering resolves it to a
        // writable location and stores the next park back, which is
        // why the signature shows no out parameter.
        "__torajs_dstr_drain_rest" | "__torajs_dstr_step_one" => {
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Any))
        }
        // 刀 D — the OWNED copy of the park the rest element takes
        // before clearing the slot (a retain; see the lowering).
        "__torajs_dstr_park_take" => Type::Function(vec![Type::Any], Box::new(Type::Any)),
        _ => return None,
    })
}

fn error_synth_ty(name: &str) -> Option<Type> {
    Some(match name {
        "__torajs_error_proto_install" => Type::Function(vec![Type::String], Box::new(Type::Void)),
        "__torajs_error_install_cause" => {
            Type::Function(vec![Type::Any, Type::Any], Box::new(Type::Void))
        }
        "__torajs_error_is_error" | "__torajs_is_constructor" => {
            Type::Function(vec![Type::Any], Box::new(Type::Boolean))
        }
        // RFC 20260915 — injected source's SpeciesConstructor(O,
        // %Promise%), lowered by `ssa_lower_call_injected_spec_ops`.
        "__torajs_promise_species_ctor" => Type::Function(vec![Type::Any], Box::new(Type::Any)),
        "__torajs_undef_str" => Type::Function(Vec::new(), Box::new(Type::String)),
        _ => return None,
    })
}

/// RFC 20260917-class-eval-unit — the per-evaluation class family,
/// kept together the way `dstr_synth_ty` keeps the pattern one: every
/// member runs inside `__cunit_<C>` on the class object that
/// evaluation minted.
fn unit_synth_ty(name: &str) -> Option<Type> {
    let void_fn = |params: Vec<Type>| Type::Function(params, Box::new(Type::Void));
    Some(match name {
        // K1 — `(class, proto, "C", parent class | undefined)`.
        UNIT_WIRE => void_fn(vec![Type::Any, Type::Any, Type::String, Type::Any]),
        // K4-1 / K4-3 — `("C", member, the evaluation's class)`.
        UNIT_STATIC_METHOD_REIFY | UNIT_STATIC_ACCESSOR_REIFY => {
            void_fn(vec![Type::String, Type::String, Type::Any])
        }
        // K4-2 — `(the evaluation's class, field name, initializer)`.
        UNIT_STATIC_FIELD_DEFINE => void_fn(vec![Type::Any, Type::String, Type::Any]),
        // K5-3 — `("C", the running this)` answers C's evaluation.
        UNIT_HOME => Type::Function(vec![Type::String, Type::Any], Box::new(Type::Any)),
        _ => return None,
    })
}

pub(crate) fn try_type(name: &str) -> Option<Result<Type, String>> {
    let t = match name {
        // RFC 20260805-async-fn-state-machine blade 1 — the injected
        // driver's PromiseResolve(%Promise%, v) (§27.7.5.3 step 2).
        // Registered here so the bare callee read types as a function
        // instead of landing in `undeclared_reads` (which lowers to a
        // global-object read and a runtime ReferenceError); the call
        // itself is typed off this function type by the general call
        // path and lowered by `ssa_lower_call_await_resolve`.
        crate::ssa_lower_call_await_resolve::NAME => Ok(Type::Function(
            vec![Type::Any],
            Box::new(Type::Promise(Box::new(Type::Any))),
        )),
        // RFC 20260914 — a self-hosted static's startup registration
        // (`__torajs_ns_static_bind(id, f)`), lowered by
        // `ssa_lower_call_ns_static_bind`.
        crate::ssa_lower_call_ns_static_bind::NAME => Ok(Type::Function(
            vec![Type::Number, Type::Any],
            Box::new(Type::Void),
        )),
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
        // RFC 20260825-inject-narrow-define 刀 4a — the prologue's
        // proto-chain wire (`__proto_<Sub>` → `__proto_<Super>`).
        "__torajs_proto_link_fresh" => Ok(Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::Void),
        )),
        "__torajs_class_register" => Ok(Type::Function(
            vec![Type::Any, Type::String, Type::Number],
            Box::new(Type::Void),
        )),
        // a Super Reference's read (base, key, receiver), a private field's
        // define, a call on a callee's receiver: every slot any-world
        "__torajs_super_prop_get" | FIELD_DEFINE | "__torajs_call_with_this" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::Any],
            Box::new(Type::Any),
        )),
        "__torajs_super_prop_set" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::Any, Type::Any],
            Box::new(Type::Any),
        )),
        "__torajs_super_prop_call" | "__torajs_super_prop_opt_call" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::Any, Type::Any],
            Box::new(Type::Any),
        )),
        // §13.4.4–13.4.7 update through a Super Reference — four
        // spellings (inc / dec × postfix / prefix), one kernel; the
        // flags ride the callee name, so the value operands are the
        // read's three.
        "__torajs_super_prop_incr_post"
        | "__torajs_super_prop_incr_pre"
        | "__torajs_super_prop_decr_post"
        | "__torajs_super_prop_decr_pre" => Ok(Type::Function(
            vec![Type::Any, Type::Any, Type::Any],
            Box::new(Type::Any),
        )),
        "__torajs_static_method_reify" => Ok(Type::Function(
            vec![Type::String, Type::String],
            Box::new(Type::Void),
        )),
        // L3b static-field-reflect: third arg is the `__sf_<C>__<f>`
        // global's value.
        "__torajs_static_field_reify" => Ok(Type::Function(
            vec![Type::String, Type::String, Type::Any],
            Box::new(Type::Void),
        )),
        "__torajs_class_accessor_reify" | "__torajs_class_static_accessor_reify" => Ok(
            Type::Function(vec![Type::String, Type::String], Box::new(Type::Void)),
        ),
        // 563-03 — (class, static member): move that own entry of the
        // class object behind everything defined so far.
        "__torajs_class_static_own_move_to_end" => Ok(Type::Function(
            vec![Type::String, Type::String],
            Box::new(Type::Void),
        )),
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
        // The same §7.1.19 shell under the name the destructuring
        // rest lane spells it with — a key evaluated at its own
        // position in the pattern, answered as the `any` both the
        // load and the excluded list then read.
        "__torajs_class_computed_key" | "__torajs_to_property_key" => {
            Ok(Type::Function(vec![Type::Any], Box::new(Type::Any)))
        }
        // 628-05 — (class, the named field the computed one follows,
        // its key global): the own-key order anchor register.
        "__torajs_class_computed_field_register" => Ok(Type::Function(
            vec![Type::String, Type::String, Type::Any],
            Box::new(Type::Void),
        )),
        // §13.15.5.4 — object rest whose excluded set has a computed
        // key in it: (source, spelled names, computed keys) -> the
        // rest object. `any`, because the shape is not a static
        // answer once one of the subtracted keys is a runtime value.
        "__torajs_obj_rest" => Ok(Type::Function(
            vec![Type::Any, Type::String, Type::Any],
            Box::new(Type::Any),
        )),
        n if dstr_synth_ty(n).is_some() => Ok(dstr_synth_ty(n).unwrap()),
        n if unit_synth_ty(n).is_some() => Ok(unit_synth_ty(n).unwrap()),
        n if error_synth_ty(n).is_some() => Ok(error_synth_ty(n).unwrap()),
        // the derived-ctor this-binding ReferenceError raiser the class
        // desugar plants (uninitialized read / second `super()`)
        "__torajs_this_binding_throw" => {
            Ok(Type::Function(vec![Type::Boolean], Box::new(Type::Boolean)))
        }
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
        // RFC 20260916-private-element-brand slice 2a.
        "__torajs_priv_install" => Ok(Type::Function(
            vec![Type::Any, Type::Number, Type::Number],
            Box::new(Type::Void),
        )),
        // RFC 20260917-plain-fn-new-target knife 3.
        "__torajs_new_target_register" => Ok(Type::Function(
            vec![Type::Any, Type::Any],
            Box::new(Type::Void),
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
