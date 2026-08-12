//! M5.N — builtin heritage: `class C extends Object` (§19.1.1).
//!
//! A parent name that is not any declared class but names a
//! subclassable builtin is not a forward reference. The Object
//! constructor is explicitly designed to be subclassable (spec
//! §19.1.1), and under an active newTarget its [[Construct]] is
//! exactly OrdinaryCreateFromConstructor — which is what tr's
//! base-class factory already does (fresh instance, prototype chain
//! `C.prototype` → `Object.prototype`, `instanceof Object` true).
//! So the class lowers as a BASE class, with two seams handled
//! here before any other class pass runs:
//!
//! - `super(...)` sites in an explicit ctor rewrite to a comma
//!   chain evaluating the arguments left-to-right for effects
//!   (§13.3.7.1 ArgumentListEvaluation still runs — a poisoned
//!   argument still throws), result `undefined`; Object contributes
//!   no per-instance state.
//! - an explicit ctor with ZERO super() sites gets the
//!   `__torajs_ctor_no_super_throw()` raiser appended (§9.2.2
//!   this-TDZ, the `append_no_super_throw` shape) — that pass keys
//!   on `parent.is_some()` and would skip the stripped entry.
//!
//! Recorded boundaries (loud or registered, not silent-new):
//! `super.m()` in a stripped class keeps its `__supercall__` spelling
//! and fails loudly downstream; the class object's own [[Prototype]]
//! (spec: `Object.getPrototypeOf(C) === Object`) stays the base-class
//! shape; ctor return-override semantics (§9.2.2 step 13) are the
//! same pre-existing face user derived classes have.
//!
//! Array / RegExp / Promise / Iterator parents each need their own
//! exotic-instance substrate and join this table one by one.

use super::desugar_classes_super::ClassIndexEntry;
use super::super_collect::collect_super_in_stmt;
use super::*;

/// Builtins accepted as an `extends` parent today. Instances stay
/// ordinary `Tag::Obj` cells; Iterator additionally chains the
/// class's prototype to %Iterator.prototype% (see
/// `builtin_proto_heir_tag`).
const SUBCLASSABLE_BUILTINS: &[&str] = &["Object", "Iterator"];

/// Builtins whose instances are exotic objects — the subclass mints a
/// REAL exotic cell via the per-builtin subclass-alloc kernel (RFC
/// 20260730 blades 1-2). Recorded in `ast.exotic_parent`; the factory
/// and `super(...)` lower differently, everything else takes the
/// stripped base-class shape below.
const EXOTIC_SUBCLASSABLE: &[&str] = &[
    "Array", "Number", "String", "Boolean", "Function", "Map", "Set", "Promise", "RegExp",
];

/// The factory's zero-arg mint magic for an exotic parent (the class
/// resolves from the enclosing `__new_<C>` fn name at lower time).
pub(crate) fn exotic_alloc_self_magic(parent: &str) -> &'static str {
    match parent {
        "Array" => "__torajs_arr_subclass_alloc_self",
        "Number" => "__torajs_number_wrapper_subclass_alloc_self",
        "String" => "__torajs_string_wrapper_subclass_alloc_self",
        "Boolean" => "__torajs_boolean_wrapper_subclass_alloc_self",
        "Function" => "__torajs_function_subclass_alloc_self",
        "Map" => "__torajs_map_subclass_alloc_self",
        "Set" => "__torajs_set_subclass_alloc_self",
        "Promise" => "__torajs_promise_subclass_alloc_self",
        "RegExp" => "__torajs_regex_subclass_alloc_self",
        _ => unreachable!("not an exotic subclassable builtin: {parent}"),
    }
}

/// The ctor-side one-argument `super(v)` semantics kernel — `new
/// Array(len)` length semantics (§23.1.2.1) / the wrapper ctors'
/// `[[*Data]] = To*(v)` coercion (§21.1.1.1 / §22.1.1.1 / §20.3.1.1).
/// `None` = no one-argument form exists for this parent yet
/// (`Function`'s body-source form needs dynamic compilation — the
/// eval-shape seam; Map/Set's iterable seeding (§24.1.1.1
/// AddEntriesFromIterable) is its own later seam) — those forms stay
/// in the loud bucket.
fn exotic_super_kernel(parent: &str) -> Option<&'static str> {
    match parent {
        "Array" => Some("__torajs_arr_subclass_super_len"),
        "Number" => Some("__torajs_number_wrapper_subclass_super"),
        "String" => Some("__torajs_string_wrapper_subclass_super"),
        "Boolean" => Some("__torajs_boolean_wrapper_subclass_super"),
        "Promise" => Some("__torajs_promise_subclass_super"),
        "RegExp" => Some("__torajs_regex_subclass_super"),
        // Rotation 371 — the collection ctors' §24.1.1.1 / §24.2.1.1
        // iterable walk applied to the minted subclass cell.
        "Map" => Some("__torajs_map_subclass_super"),
        "Set" => Some("__torajs_set_subclass_super"),
        "Function" => None,
        _ => unreachable!("not an exotic subclassable builtin: {parent}"),
    }
}

/// Whether a bare `super()` is the builtin's no-argument ctor (a
/// no-op against the mint's default). Promise is the exception:
/// §27.2.3.1 step 2 rejects a non-callable executor, so `super()`
/// routes to the kernel with undefined and takes the TypeError.
fn exotic_super_zero_is_noop(parent: &str) -> bool {
    parent != "Promise"
}

/// SUBCLASSABLE (stripped, ordinary-instance) parents whose only
/// substrate contribution is a prototype-chain link: the class's
/// `__proto_<C>` dynobj gets a PROTO_SLOT_KEY entry pointing at the
/// builtin prototype singleton of the returned tag, so
/// `new C() instanceof Iterator` and inherited-helper dispatch walk
/// through it. `None` = no link needed (Object — the default chain
/// is already correct). RFC 20260730-iterator-global 刀 1.
fn builtin_proto_heir_tag(parent: &str) -> Option<i64> {
    match parent {
        "Iterator" => Some(15),
        _ => None,
    }
}

/// Strip a builtin parent down to base-class shape (see module doc).
/// Runs on the mutable `class_index` FIRST — before default-ctor
/// synthesis (a stripped class takes the base default ctor, not the
/// derived super-forwarding one) and before the forward-reference
/// validation in `compute_full_fields` (which would reject the
/// builtin name).
pub(super) fn strip_builtin_heritage(ast: &mut Ast, class_index: &mut [ClassIndexEntry]) {
    let declared: std::collections::HashSet<String> =
        class_index.iter().map(|e| e.1.clone()).collect();
    for (_, cname, _tp, parent, fields, _, ctor, _, _) in class_index.iter_mut() {
        let Some(p) = parent.as_ref() else { continue };
        // A user class of the same name shadows the builtin — the
        // ordinary declared-parent path handles it.
        if declared.contains(p) {
            continue;
        }
        let exotic = EXOTIC_SUBCLASSABLE.contains(&p.as_str());
        if !exotic && !SUBCLASSABLE_BUILTINS.contains(&p.as_str()) {
            continue;
        }
        if exotic {
            if !fields.is_empty() {
                // Exotic cells have no fixed field region — declared
                // fields go dict-mode through the expando face, a
                // later blade. Loud until then (same bucket as M5.2).
                panic!(
                    "M5.N: `{cname} extends {p}` — declared fields on an exotic builtin \
                     subclass are not yet supported"
                );
            }
            ast.exotic_parent.insert(cname.clone(), p.clone());
        }
        if let Some(tag) = builtin_proto_heir_tag(p) {
            ast.builtin_proto_heirs.insert(cname.clone(), tag);
        }
        // Rotation 371 — a ctor-less exotic subclass gets the spec
        // derived default ctor's observable half: `new MySet(iter)`
        // must hand its argument to the builtin's [[Construct]]
        // (probe: the iterable silently dropped and the set came up
        // empty). The general default-ctor synthesis pass runs AFTER
        // this strip and keys on `parent.is_some()`, so the stripped
        // entry never gets one — synthesize the single-argument
        // forward here. Map / Set ONLY: their kernels treat a
        // nullish argument as the no-op §24.x.1.1 step 6, so
        // `super(undefined)` from a 0-argument `new` is exactly
        // `super()`; Array / the wrappers are NOT argument-count
        // agnostic (`new Array(undefined)` is a RangeError,
        // `new Number(undefined)` is NaN where `new Number()` is +0
        // — the db66228e gate red), so they keep the no-forward
        // shape until an arguments-length-aware forward exists
        // (rest-forwarding is the recorded call-spread boundary,
        // L3b 371-01).
        if exotic && ctor.is_none() && matches!(p.as_str(), "Map" | "Set") {
            let arg = ast.add_expr(Expr::Ident("__superarg".to_string()));
            let sup = ast.add_expr(Expr::Super { args: vec![arg] });
            *ctor = Some(crate::ast::ClassCtor {
                params: vec![crate::ast::Param {
                    name: "__superarg".to_string(),
                    type_ann: Some("any".to_string()),
                    default: None,
                    is_rest: false,
                }],
                body: vec![Stmt::Expr(sup)],
            });
        }
        if let Some(c) = ctor.as_mut() {
            let mut sites: Vec<(ExprId, Vec<ExprId>)> = Vec::new();
            for s in &c.body {
                collect_super_in_stmt(ast, s, &mut sites);
            }
            if sites.is_empty() && ast.explicit_ctor_classes.contains(cname) {
                let callee = ast.add_expr(Expr::Ident("__torajs_ctor_no_super_throw".to_string()));
                let call = ast.add_expr(Expr::Call {
                    callee,
                    args: Vec::new(),
                });
                c.body.push(Stmt::Expr(call));
            }
            for (eid, args) in sites {
                if exotic {
                    // The builtin's [[Construct]] under an active
                    // newTarget: `super()` contributes nothing beyond
                    // the minted cell (each mint already carries the
                    // no-argument default); `super(v)` applies the
                    // builtin ctor's semantics to it where a
                    // one-argument form exists. The rest — multi-arg
                    // (`super(a, b, ...)`) and Function's body-source
                    // compile — are later seams, loud in the same
                    // not-yet-supported bucket as M5.2.
                    match args.len() {
                        0 if exotic_super_zero_is_noop(p) => {
                            ast.exprs[eid.0 as usize] = Expr::Ident("undefined".into());
                        }
                        0 | 1 if exotic_super_kernel(p).is_some() => {
                            let callee = ast
                                .add_expr(Expr::Ident(exotic_super_kernel(p).unwrap().to_string()));
                            let this_id = ast.add_expr(Expr::This);
                            let arg = args.first().copied().unwrap_or_else(|| {
                                ast.add_expr(Expr::Ident("undefined".to_string()))
                            });
                            ast.exprs[eid.0 as usize] = Expr::Call {
                                callee,
                                args: vec![this_id, arg],
                            };
                        }
                        _ => panic!(
                            "M5.N: `{cname} extends {p}` — this super(...) argument form \
                             is not yet supported for an exotic builtin parent"
                        ),
                    }
                    continue;
                }
                if args.is_empty() {
                    ast.exprs[eid.0 as usize] = Expr::Ident("undefined".into());
                    continue;
                }
                let mut right = ast.add_expr(Expr::Ident("undefined".into()));
                for &a in args[1..].iter().rev() {
                    right = ast.add_expr(Expr::Sequence { left: a, right });
                }
                ast.exprs[eid.0 as usize] = Expr::Sequence {
                    left: args[0],
                    right,
                };
            }
        }
        *parent = None;
    }
}
