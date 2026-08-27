//! Slot-shape inference for un-annotated top-level bindings —
//! `GlobalSlotShape` and its initializer walkers, split from
//! `ast_refs.rs` at the 500-line boundary (r290).

use crate::ast::{Ast, Expr, ExprId, Stmt, UnaryOp};

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
    /// Cluster #4 follow-up (rotation 235) — a `Symbol()` /
    /// `Symbol(desc)` init: fresh heap mint per §20.4.1, so the
    /// slot's runtime type is statically certain like the literal
    /// shapes above (test262's forbidden-ext family reads such a
    /// binding from class-method bodies).
    Symbol,
}

pub fn infer_toplevel_slot_shape(ast: &Ast, init: ExprId) -> Option<GlobalSlotShape> {
    infer_slot_shape(ast, init, 0)
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
        _ => None,
    }
}

fn infer_slot_shape(ast: &Ast, init: ExprId, depth: u32) -> Option<GlobalSlotShape> {
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
        Expr::Ident(n) if n == "NaN" || n == "Infinity" => Some(GlobalSlotShape::F64),
        // Alias of another top-level binding: same shape as the
        // binding it copies. An annotated upstream maps through the
        // simple-ann table; an un-annotated one recurses into its
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
                    Some(ann) => shape_of_simple_ann(ann),
                    None => infer_slot_shape(ast, *init, depth + 1),
                }),
                _ => None,
            })
            .flatten(),
        Expr::Unary {
            op: UnaryOp::Neg,
            expr,
        } => infer_slot_shape(ast, *expr, depth + 1),
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

/// RFC 20260709-closure-global chunk 2 — the canonical `__fn(P|..)->R`
/// spelling for a lifted closure's FnDecl, read off its param /
/// return anns. By the time the checker / lowerer consult this,
/// `preinfer_closure_sigs` has backfilled missing param anns with
/// `any` and inferred the return ann (or left `None` for a body
/// without value returns, which spells `void` — same mapping that
/// pass uses when publishing sigs). `None` when the decl is absent or
/// a param ann is still missing (a non-`__closure_*` decl this pass
/// never touched).
pub fn lifted_closure_fn_canon(ast: &Ast, fn_name: &str) -> Option<String> {
    ast.stmts.iter().find_map(|s| match s {
        Stmt::FnDecl {
            name,
            params,
            return_type,
            ..
        } if name == fn_name => {
            let mut anns: Vec<String> = Vec::with_capacity(params.len());
            for p in params.iter().filter(|p| p.name != "__env") {
                anns.push(p.type_ann.clone()?);
            }
            let ret = match return_type {
                Some(rt) => rt.clone(),
                None => "void".to_string(),
            };
            Some(format!("__fn({})->{}", anns.join("|"), ret))
        }
        _ => None,
    })
}

/// RFC 20260725 follow-up (un-annotated struct global) — the
/// canonical `__inlobj(f:T|...)` spelling for an all-literal-field
/// ObjectLit init (`let s = { a: 1, b: "x" }`). Both the checker's
/// pass_2 registration and the lowerer's K.3b slot inference resolve
/// the SAME string through their existing annotation pipelines, so
/// the two slots cannot drift (and the interned layout unifies with
/// an equivalent written annotation). `None` — keeping the binding
/// main-local — for any field that isn't a Number/String/Bool
/// literal or a lifted arrow, a computed-key / symbol / spread /
/// dunder sentinel name, a non-identifier name (the spelling's `:` /
/// `|` separators must stay unambiguous), or an empty literal (`{}`
/// is the dynobj-family idiom, left to the degrade passes).
///
/// A method-valued field carries its lifted fn's `__fn(...)->T`
/// spelling — the one a written `{ f: (x: number) => number }`
/// annotation resolves through too, and the `|` between its params
/// sits inside parens where `split_top_pipe`'s depth counter already
/// leaves it alone. Without this arm, ONE closure field kept the
/// whole binding main-local, so a named fn reading any field of it
/// got "unknown identifier" — which is what made `const thenable =
/// { then(res) {…} }` unreachable from every function in the file.
/// Variadic sigs stay out for the same reason the sibling arms keep
/// them out: their boxed-dual routing is a fn-local table.
pub fn objlit_literal_inlobj_ann(ast: &Ast, init: ExprId) -> Option<String> {
    let Expr::ObjectLit { fields } = ast.get_expr(init) else {
        return None;
    };
    if fields.is_empty() {
        return None;
    }
    let mut parts: Vec<String> = Vec::with_capacity(fields.len());
    for (fname, val) in fields {
        let mut chars = fname.chars();
        let head_ok = chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$');
        if !head_ok
            || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
            || fname.starts_with("__")
        {
            return None;
        }
        let ann: String = match ast.get_expr(*val) {
            Expr::Number(n) => {
                if n.fract() != 0.0 || n.abs() >= 9.223372036854776e18 {
                    "f64".to_string()
                } else {
                    "number".to_string()
                }
            }
            Expr::String(_) => "string".to_string(),
            Expr::Bool(_) => "boolean".to_string(),
            Expr::Closure { fn_name, .. } => {
                let canon = lifted_closure_fn_canon(ast, fn_name)?;
                if canon.contains("__rest(") {
                    return None;
                }
                // A struct FIELD holding a fn is Closure-repr, not the
                // direct-dispatch FnSig a `__fn(` spelling names — the
                // same retag the parser applies where it mints
                // `__inlobj(` from written syntax. Leaving it untagged
                // resolves the field to a bare code pointer and calling
                // it through an env-first indirect call is a SIGBUS.
                crate::ast::retag_field_fn_ann(&canon)
            }
            _ => return None,
        };
        parts.push(format!("{fname}:{ann}"));
    }
    Some(format!("__inlobj({})", parts.join("|")))
}

/// The class spelling of an un-annotated `new C(...)` init — the
/// synthesized annotation a top-level binding promotes under, in the
/// [`objlit_literal_inlobj_ann`] / `arrlit_literal_elem_ann` family.
///
/// `desugar_classes` has already rewritten `new C(args)` into a call
/// to the synthesized `__new_C` factory by the time either consumer
/// asks, so the spelling is the name that factory carries. Requiring
/// the factory to EXIST is what keeps this to classes the program
/// declares: a `new` whose target the compiler could not resolve to
/// a class never got one (`NewDynamic`), and so never answers here.
///
/// The nominal type is the whole point. `any_promote_init` refuses
/// class instances deliberately — boxing one away demotes every
/// main-side method call to the any-lane (rotation 238) — but that
/// refusal left the binding with no home at all, so a named fn
/// reading it died with "unknown identifier": `let e = new Error(…)`
/// plus any `function f() { … e … }` did not compile, while the same
/// program with `let e: Error` written out did. Under its own
/// spelling the binding rides exactly the lane the written
/// annotation already rides.
pub fn new_class_ann(ast: &Ast, init: ExprId) -> Option<String> {
    let Expr::Call { callee, .. } = ast.get_expr(init) else {
        return None;
    };
    let Expr::Ident(fname) = ast.get_expr(*callee) else {
        return None;
    };
    let class = fname.strip_prefix("__new_")?;
    crate::ast::toplevel_stmts_flat(ast)
        .iter()
        .any(|s| matches!(s, Stmt::FnDecl { name, .. } if name.as_str() == fname.as_str()))
        .then(|| class.to_string())
}
