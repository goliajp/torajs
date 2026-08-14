//! Static type-inference helpers for `desugar_implicit_generics` —
//! chunk 361, extracted from ast.rs.
//!
//! Pre-pass utilities called from `ast_desugar_implicit_generics::run`:
//! given an `Ast.exprs` slice + a fn body, statically sniff a return
//! type / expression annotation string without consulting the
//! typechecker. Bail to `None` on any shape the simple grammar can't
//! resolve (Member / Call / Index / object literal fall-through etc.);
//! the deeper analysis remains the typechecker's job.
//!
//! Public surface (via `pub(crate) use` re-export in ast.rs):
//!   * `AstExprsView` — `&'a [Expr]` type alias.
//!   * `infer_return_ann` / `infer_return_ann_seeded` — return-type
//!     sniff over a body (seeded variant flows outer captures into
//!     the binding map, T-19.p).
//!   * `binds_to_params` — bindings HashMap → `Vec<Param>` for
//!     `infer_expr_ann_with` lookup.
//!   * `infer_expr_ann_with` — main expression annotation sniff
//!     (literals + BinOp + Ident + Call/Member method whitelist +
//!     Ternary + Array + Member/length + Index + ObjectLit).
//!
//! Private helpers: `collect_let_binding_anns(_stmt)`,
//! `collect_return_anns(_stmt)`, `is_typevar_ann`.
//!
//! `body_has_value_return` — the control-flow question of whether an
//! inferred return type is wanted at all — lives in the
//! `implicit_generics_value_return` sibling.

use super::{BinOp, Expr, ExprId, Param, Stmt, UnaryOp, retag_field_fn_ann};

/// Borrow-shaped view of `Ast.exprs` for the inference helper. Defined
/// at the top of `desugar_implicit_generics` (just below) — `&[Expr]`
/// indexed by `ExprId.0 as usize`. The pre-pass walks expression
/// shapes statically without consulting the typechecker, so this
/// flat slice is enough.
pub(crate) type AstExprsView<'a> = &'a [Expr];

/// Static return-type sniff. Walks every value-return inside `body`
/// (recursing through control-flow shapes that propagate value-
/// returns out of the fn) and asks `infer_expr_ann` for an annotation.
/// Returns `Some(ann)` only if every reachable return agrees; any
/// disagreement or any return that resists static typing yields None
/// (caller leaves return_type alone).
///
/// Beyond literals + boolean-result ops, the helper does a one-pass
/// scan over let-decl bodies to populate a binding → annotation map
/// (so `let x = 3; ... return x + 1;` infers `x: number` and bubbles
/// `number` out as the return). Lookups that fall off the simple-
/// shape grammar (Member / Call / Index / object literal / etc.) bail
/// to None — the typechecker still owns the deeper analysis.
pub(crate) fn infer_return_ann(
    exprs: AstExprsView,
    body: &[Stmt],
    params: &[Param],
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    infer_return_ann_seeded(
        exprs,
        body,
        params,
        &std::collections::HashMap::new(),
        fn_sigs,
    )
}

/// T-19.p — variant that takes a pre-seeded binds map. Used by the
/// capturing-closure FnDecl path so outer-scope let-decls (where
/// the captures actually live) flow into the body's static return
/// sniff. The pre-T-19.p path passed only the body's params + body
/// let-decls — captured idents had no entry in binds and the sniff
/// bailed to None. Idempotent: explicit body-local lets shadow the
/// outer seed via collect_let_binding_anns running afterwards.
pub(crate) fn infer_return_ann_seeded(
    exprs: AstExprsView,
    body: &[Stmt],
    params: &[Param],
    outer_binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let mut binds: std::collections::HashMap<String, String> = outer_binds.clone();
    for p in params {
        if let Some(ann) = &p.type_ann {
            binds.insert(p.name.clone(), ann.clone());
        }
    }
    collect_let_binding_anns(exprs, body, &mut binds, fn_sigs);
    let mut acc: Option<String> = None;
    if !collect_return_anns(exprs, body, &binds, fn_sigs, &mut acc) {
        return None;
    }
    acc
}

/// Walk `body` and for each `let x = INIT;` whose `INIT` we can
/// statically annotate, register `x → ann` in `binds`. Inner
/// FnDecls / nested classes etc. are skipped — we only care about
/// the immediate fn's binding scope. Idempotent over re-runs since
/// later let-decls in the same scope shadow earlier ones.
fn collect_let_binding_anns(
    exprs: AstExprsView,
    body: &[Stmt],
    binds: &mut std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) {
    for s in body {
        collect_let_binding_anns_stmt(exprs, s, binds, fn_sigs);
    }
}

fn collect_let_binding_anns_stmt(
    exprs: AstExprsView,
    s: &Stmt,
    binds: &mut std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) {
    match s {
        Stmt::LetDecl {
            name,
            type_ann,
            init,
            ..
        } => {
            // Explicit annotation wins.
            if let Some(ann) = type_ann {
                binds.insert(name.clone(), ann.clone());
                return;
            }
            // Else infer from init shape — using the binds map we've
            // built up to here, so `let x = 3; let y = x + 1;` chains
            // correctly.
            let bs: Vec<Param> = binds_to_params(binds);
            if let Some(ann) = infer_expr_ann_with(exprs, *init, &bs, binds, fn_sigs) {
                binds.insert(name.clone(), ann);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            collect_let_binding_anns_stmt(exprs, then_branch, binds, fn_sigs);
            if let Some(eb) = else_branch {
                collect_let_binding_anns_stmt(exprs, eb, binds, fn_sigs);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_let_binding_anns_stmt(exprs, body, binds, fn_sigs);
        }
        Stmt::Labeled { body, .. } => {
            collect_let_binding_anns_stmt(exprs, body, binds, fn_sigs);
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                collect_let_binding_anns_stmt(exprs, i, binds, fn_sigs);
            }
            collect_let_binding_anns_stmt(exprs, body, binds, fn_sigs);
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            collect_let_binding_anns(exprs, stmts, binds, fn_sigs);
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            collect_let_binding_anns(exprs, body, binds, fn_sigs);
            collect_let_binding_anns(exprs, catch_body, binds, fn_sigs);
            if let Some(fb) = finally_body {
                collect_let_binding_anns(exprs, fb, binds, fn_sigs);
            }
        }
        _ => {}
    }
}

pub(crate) fn binds_to_params(binds: &std::collections::HashMap<String, String>) -> Vec<Param> {
    binds
        .iter()
        .map(|(k, v)| Param {
            name: k.clone(),
            type_ann: Some(v.clone()),
            default: None,
            is_rest: false,
        })
        .collect()
}

/// Returns false on first disagreement / un-inferable return; on
/// success, `acc` holds the unique annotation across all returns.
fn collect_return_anns(
    exprs: AstExprsView,
    body: &[Stmt],
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
    acc: &mut Option<String>,
) -> bool {
    for s in body {
        if !collect_return_anns_stmt(exprs, s, binds, fn_sigs, acc) {
            return false;
        }
    }
    true
}

fn collect_return_anns_stmt(
    exprs: AstExprsView,
    s: &Stmt,
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
    acc: &mut Option<String>,
) -> bool {
    match s {
        Stmt::Return(Some(eid)) => {
            let bs = binds_to_params(binds);
            let Some(ann) = infer_expr_ann_with(exprs, *eid, &bs, binds, fn_sigs) else {
                return false;
            };
            match acc {
                None => *acc = Some(ann),
                Some(prev) if *prev == ann => {}
                Some(_) => return false,
            }
            true
        }
        Stmt::Return(None) => true,
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            if !collect_return_anns_stmt(exprs, then_branch, binds, fn_sigs, acc) {
                return false;
            }
            if let Some(eb) = else_branch
                && !collect_return_anns_stmt(exprs, eb, binds, fn_sigs, acc)
            {
                return false;
            }
            true
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } => {
            collect_return_anns_stmt(exprs, body, binds, fn_sigs, acc)
        }
        Stmt::Labeled { body, .. } => collect_return_anns_stmt(exprs, body, binds, fn_sigs, acc),
        Stmt::For { init, body, .. } => {
            if let Some(i) = init
                && !collect_return_anns_stmt(exprs, i, binds, fn_sigs, acc)
            {
                return false;
            }
            collect_return_anns_stmt(exprs, body, binds, fn_sigs, acc)
        }
        Stmt::Block(stmts) | Stmt::Multi(stmts) => {
            collect_return_anns(exprs, stmts, binds, fn_sigs, acc)
        }
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => {
            if !collect_return_anns(exprs, body, binds, fn_sigs, acc) {
                return false;
            }
            if !collect_return_anns(exprs, catch_body, binds, fn_sigs, acc) {
                return false;
            }
            if let Some(fb) = finally_body
                && !collect_return_anns(exprs, fb, binds, fn_sigs, acc)
            {
                return false;
            }
            true
        }
        // Switch / nested FnDecl etc. — conservative: treat as opaque,
        // make the whole inference bail (returns are uncommon inside
        // these shapes for our test262 surface).
        Stmt::FnDecl { .. } => true,
        _ => true,
    }
}

fn is_typevar_ann(s: &str) -> bool {
    s.starts_with("__T") && s.len() > 3 && s[3..].bytes().all(|b| b.is_ascii_digit())
}

/// Statically infer the type-annotation string for an expression (best-effort).
pub(crate) fn infer_expr_ann_with(
    exprs: AstExprsView,
    eid: ExprId,
    params: &[Param],
    binds: &std::collections::HashMap<String, String>,
    fn_sigs: &std::collections::HashMap<String, String>,
) -> Option<String> {
    let recur = |id: ExprId| infer_expr_ann_with(exprs, id, params, binds, fn_sigs);
    let e = exprs.get(eid.0 as usize)?;
    match e {
        Expr::Number(_) => Some("number".into()),
        Expr::String(_) => Some("string".into()),
        Expr::Bool(_) => Some("boolean".into()),
        Expr::BinOp { op, left, right } => match op {
            BinOp::Lt
            | BinOp::Gt
            | BinOp::Le
            | BinOp::Ge
            | BinOp::Eq
            | BinOp::Neq
            | BinOp::LooseEq
            | BinOp::LooseNeq
            | BinOp::LAnd
            | BinOp::LOr => Some("boolean".into()),
            BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr
            | BinOp::UShr => Some("number".into()),
            // `+`: string-propagates / num+num=num / TypeVar+num=`any` (else Void).
            BinOp::Add => {
                let (l, r) = (recur(*left)?, recur(*right)?);
                match (l.as_str(), r.as_str()) {
                    ("string", _) | (_, "string") => Some("string".into()),
                    ("number", "number") => Some("number".into()),
                    (a, "number") | ("number", a) if is_typevar_ann(a) => Some("any".into()),
                    _ => None,
                }
            }
        },
        Expr::Unary { op, .. } if matches!(op, UnaryOp::Not) => Some("boolean".into()),
        Expr::Unary { .. } => Some("number".into()),
        Expr::Ident(name) => params
            .iter()
            .find(|p| &p.name == name)
            .and_then(|p| p.type_ann.clone())
            .or_else(|| binds.get(name).cloned()),
        // RFC 20260704 C4+ (chunk 522) — a lifted closure value
        // carries the fn-shaped ann `preinfer_closure_sigs`
        // published under its reserved `__closure_*` name (a full
        // `__fn(P|..)->R`, unlike the bare return anns the map
        // holds for user fns; the namespaces are disjoint — user
        // code never references a `__closure_*` ident directly).
        Expr::Closure { fn_name, .. } => fn_sigs.get(fn_name).cloned(),
        // Call: bare Ident → fn_sigs; Member+string/`T[]` receiver → method whitelist.
        // Omitted methods SIGSEGV/silent-wrong even with explicit annotation (L3b).
        Expr::Call { callee, .. } => {
            let c = exprs.get(callee.0 as usize)?;
            if let Expr::Ident(n) = c {
                // The parser's synthetic relational calls answer Bool
                // (`in` / the §13.10 `#x in o` brand check) — they
                // are not user fns, so fn_sigs can't know them, and a
                // bare `return #x in o` would bail the sniff to Void
                // (the mono body then hands unboxed garbage back
                // through an any ret).
                if n == "__torajs_in_op" || n == "__torajs_priv_in_op" {
                    return Some("boolean".to_string());
                }
                return fn_sigs.get(n).cloned();
            }
            if let Expr::Member { obj, name } = c {
                let r = recur(*obj)?;
                let elem = r.strip_suffix("[]");
                let ret: &str = match (r.as_str(), elem, name.as_str()) {
                    (
                        "string",
                        _,
                        "toUpperCase" | "toLowerCase" | "trim" | "trimStart" | "trimEnd" | "slice"
                        | "substring" | "repeat" | "concat" | "replace" | "replaceAll" | "padStart"
                        | "padEnd" | "at" | "charAt",
                    ) => "string",
                    ("string", _, "startsWith" | "endsWith" | "includes") => "boolean",
                    ("string", _, "indexOf" | "lastIndexOf" | "charCodeAt") => "number",
                    // RFC 20260705 chunk 557 — conversion methods on known
                    // primitive/array receivers (`j.toString()` inside a
                    // string-concat chain bailed the whole sniff → Void).
                    // Receivers whose ann we can't resolve still bail —
                    // user classes may override toString with another shape.
                    ("number" | "boolean" | "string", _, "toString" | "toLocaleString") => "string",
                    ("number", _, "toFixed" | "toPrecision" | "toExponential") => "string",
                    (_, Some(_), "toString" | "toLocaleString") => "string",
                    (_, Some(e), "pop" | "shift" | "at" | "find" | "findLast") => e,
                    (
                        _,
                        Some(_),
                        "slice" | "map" | "reverse" | "sort" | "concat" | "fill" | "filter"
                        | "flat" | "flatMap",
                    ) => &r,
                    (_, Some(_), "every" | "some" | "includes") => "boolean",
                    (_, Some(_), "indexOf" | "lastIndexOf" | "findIndex" | "findLastIndex") => {
                        "number"
                    }
                    (_, Some(_), "join") => "string",
                    // `reduce` / `reduceRight` return type is the callback's
                    // accumulator type — for the common case where the
                    // accumulator is the same shape as elements (e.g.
                    // `xs.reduce((a, b) => a + b, 0)` on Number[]), it
                    // matches the element type. Mixed-type accumulators
                    // (cb returns a different shape than elem) still need
                    // explicit ret annotation (substrate follow-up).
                    (_, Some(e), "reduce" | "reduceRight") => e,
                    _ => return None,
                };
                return Some(ret.to_string());
            }
            None
        }
        Expr::Ternary {
            then_branch,
            else_branch,
            ..
        } => {
            let (a, b) = (recur(*then_branch)?, recur(*else_branch)?);
            if a == b { Some(a) } else { Some("any".into()) }
        }
        // Heterogeneous literals widen to `any[]` (mirrors the
        // checker's T-10.c anchor widening — answering the first
        // element's type for `["use", x]` stamps a `string[]` ret the
        // literal's Arr<Any> lowering can't satisfy: NULL reads on
        // the named-fn lane, a runtime elem-type throw on the
        // closure lane). An element whose type can't be inferred
        // widens the same way rather than guessing.
        Expr::Array(elems) => {
            let first = recur(*elems.first()?)?;
            let uniform = elems
                .iter()
                .skip(1)
                .all(|e| recur(*e).as_deref() == Some(first.as_str()));
            Some(format!("{}[]", if uniform { first } else { "any".into() }))
        }
        Expr::Member { obj, name } if name == "length" => recur(*obj)
            .filter(|r| r == "string" || r.ends_with("[]"))
            .map(|_| "number".into()),
        Expr::Index { obj, .. } => recur(*obj)?.strip_suffix("[]").map(str::to_string),
        // The second site that mints `__inlobj(` (the parser's syntax
        // lane is the other). A fn-valued field arrives here as
        // `__fn(P)->R` (the `Expr::Closure` arm above hands back the
        // lifted closure's published sig), so it needs the same
        // field-position retag the parser does — `function make() {
        // return { f: () => 7 } }` interned a bare-fn-ptr slot while
        // the literal stored a closure env block, and the field call
        // CallIndirect'd into the env header (SIGBUS).
        Expr::ObjectLit { fields } => {
            // A computed key has no static name: the parser left a
            // `__computed_<n>__` sentinel where the name goes and
            // parked the key expression in a side table, and only the
            // dynobj lane can ToPropertyKey it. Spelling the sentinel
            // into an `__inlobj(` shape hands the caller a struct
            // whose fields are neither the ones the object has nor in
            // the order it has them, while the value itself lowers
            // through the dynobj lane — `function mk(k: string) {
            // return { [k]: 1, z: 0 } }` answered `r["a"] === 1` and
            // `Object.keys(r) === ["a","z"]` but serialized as
            // `{"__computed_0__":0,"z":0}`, a wrong answer with
            // nothing loud about it. `any` is the same exit the
            // checker takes for such a literal at every other
            // position.
            if fields.iter().any(|(n, _)| n.starts_with("__computed_")) {
                return Some("any".to_string());
            }
            fields
                .iter()
                .map(|(n, eid)| recur(*eid).map(|t| format!("{n}:{}", retag_field_fn_ann(&t))))
                .collect::<Option<Vec<_>>>()
                .map(|p| format!("__inlobj({})", p.join("|")))
        }
        _ => None,
    }
}
