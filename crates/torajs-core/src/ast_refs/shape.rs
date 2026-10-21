//! Slot-shape inference for un-annotated top-level bindings —
//! `GlobalSlotShape` and its initializer walkers, split from
//! `ast_refs.rs` at the 500-line boundary (r290).
//!
//! Which primitive SLOT TYPE an initializer has is the whole subject
//! here. What canonical ANNOTATION STRING it registers under is the
//! sibling's ([`super::init_ann`]), which moved out at the same
//! boundary in r602.

use crate::ast::{Ast, Expr, ExprId, Stmt};

/// Slot type for an un-annotated top-level declaration, recovered from
/// initializer shapes whose runtime type is statically certain. Number
/// width must match what lowering actually produces for the same
/// expression — a guess in the wrong direction stores f64 bits in an
/// i64 slot (garbage on every read), so anything uncertain returns
/// None and the binding stays a main-fn local.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalSlotShape {
    I64,
    F64,
    Str,
    Bool,
    /// A regex literal init (rotation 645): a fresh RegExp per
    /// evaluation (§13.2.7.3), its runtime type as certain as the
    /// literal shapes above. The test262 harness keeps
    /// `__t262_ASCII_IDENTIFIER = /…/u` at the top level and reads it
    /// from `formatPropertyName` — a named fn — which was an
    /// `unknown identifier` for every case that reached it.
    RegExp,
    /// Cluster #4 follow-up (rotation 235) — a `Symbol()` /
    /// `Symbol(desc)` init: fresh heap mint per §20.4.1, so the
    /// slot's runtime type is statically certain like the literal
    /// shapes above (test262's forbidden-ext family reads such a
    /// binding from class-method bodies).
    Symbol,
    /// 468-01 remainder (rotation 542) — a BigInt literal or a
    /// BigInt-only arithmetic result. Its runtime type is as certain
    /// as `Str`'s and carries no width question, and the slot
    /// machinery already admits it (`slot_type_supported` takes every
    /// refcounted type since rotation 514, `emit_drop_value` has a
    /// `bigint_drop_rc` arm). Absent this variant `const b = 2n` plus
    /// `function f() { return b }` answered "unknown identifier" and
    /// threw at runtime.
    BigInt,
}

pub fn infer_toplevel_slot_shape(ast: &Ast, init: ExprId) -> Option<GlobalSlotShape> {
    infer_slot_shape(ast, init, 0, None)
}

/// An annotated top-level binding whose annotation does not say what it
/// holds, asked as `(annotation, initializer)`. The walk answers None
/// through such a binding, as for any other uncertain hop; only the
/// checker knows which bindings these are.
pub type HeldUpstream<'a> = Option<&'a dyn Fn(&str, ExprId) -> bool>;

/// [`infer_toplevel_slot_shape`], with the bindings `held` names
/// counted as uncertain.
pub fn infer_toplevel_slot_shape_held(
    ast: &Ast,
    init: ExprId,
    held: HeldUpstream<'_>,
) -> Option<GlobalSlotShape> {
    infer_slot_shape(ast, init, 0, held)
}

/// Alias resolution is depth-capped: each hop is one top-level `let`
/// lookup, and a cycle (`const a = b; const b = a`) would otherwise
/// recurse forever. Eight hops covers any hand-written chain; deeper
/// just keeps the main-fn-local behavior.
const MAX_ALIAS_DEPTH: u32 = 8;

/// The four annotation spellings whose slot shape is statically
/// certain — the same table the named-fn-call arm has always used
/// for return annotations. Anything else (any / unions / containers)
/// answers None and the binding stays a main-fn local.
fn shape_of_simple_ann(ann: &str) -> Option<GlobalSlotShape> {
    match ann {
        "number" | "i64" => Some(GlobalSlotShape::I64),
        "f64" => Some(GlobalSlotShape::F64),
        "string" => Some(GlobalSlotShape::Str),
        "boolean" => Some(GlobalSlotShape::Bool),
        "bigint" => Some(GlobalSlotShape::BigInt),
        _ => None,
    }
}

pub(super) fn infer_slot_shape(
    ast: &Ast,
    init: ExprId,
    depth: u32,
    held: HeldUpstream<'_>,
) -> Option<GlobalSlotShape> {
    if depth > MAX_ALIAS_DEPTH {
        return None;
    }
    match ast.get_expr(init) {
        // Mirrors infer_arg_width: genuinely fractional or past i64
        // range must be f64; integral literals take the i64 default.
        Expr::Number(n) => Some(if n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 {
            GlobalSlotShape::F64
        } else {
            GlobalSlotShape::I64
        }),
        Expr::String(_) => Some(GlobalSlotShape::Str),
        Expr::Bool(_) => Some(GlobalSlotShape::Bool),
        Expr::BigInt { .. } => Some(GlobalSlotShape::BigInt),
        Expr::Regex { .. } => Some(GlobalSlotShape::RegExp),
        Expr::Ident(n) if n == "NaN" || n == "Infinity" => Some(GlobalSlotShape::F64),
        // Alias of another top-level binding: same shape as the
        // binding it copies. An annotated upstream maps through the
        // simple-ann table, unless it holds a value its annotation
        // does not describe; an un-annotated one recurses into its
        // init. Anything unresolved (fn names, missing decls, deep
        // chains) keeps the main-fn-local behavior.
        Expr::Ident(n) => ast
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::LetDecl {
                    name,
                    type_ann,
                    init,
                    ..
                } if name == n => Some(match type_ann.as_deref() {
                    Some(ann) if held.is_some_and(|h| h(ann, *init)) => None,
                    Some(ann) => shape_of_simple_ann(ann),
                    None => infer_slot_shape(ast, *init, depth + 1, held),
                }),
                _ => None,
            })
            .flatten(),
        // §13.16 — the comma operator's value is its right operand;
        // the left is evaluated for effect at the declaration
        Expr::Sequence { right, .. } => infer_slot_shape(ast, *right, depth, held),
        // Every operator arm — the ones whose answer comes from the
        // operator rather than from the value — lives in the sibling
        // module. See [`crate::ast_refs::shape_operator`].
        Expr::BinOp { .. } | Expr::Unary { .. } => {
            super::shape_operator::infer_operator_shape(ast, init, depth, held)
        }
        // Call to a top-level named fn: the return annotation is the
        // same ground truth lowering uses for the callee's ret slot.
        Expr::Call { callee, .. } => {
            let Expr::Ident(fname) = ast.get_expr(*callee) else {
                return None;
            };
            let fn_decl_shape = ast.stmts.iter().find_map(|s| match s {
                Stmt::FnDecl {
                    name,
                    return_type,
                    params,
                    ..
                } if name == fname && params.first().is_none_or(|p| p.name != "__env") => {
                    Some(return_type.as_deref().and_then(shape_of_simple_ann))
                }
                _ => None,
            });
            match fn_decl_shape {
                Some(shape) => shape,
                // Cluster #4 follow-up — a `Symbol()` ctor call with
                // no user FnDecl shadowing the name mints a fresh
                // Symbol cell (§20.4.1): statically certain shape.
                None if fname == "Symbol" => Some(GlobalSlotShape::Symbol),
                None => None,
            }
        }
        _ => None,
    }
}
