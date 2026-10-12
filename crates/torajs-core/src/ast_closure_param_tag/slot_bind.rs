//! Reads of a binding a slot read initializes, resolved by scope for
//! [`crate::ast_closure_param_tag`]. A fn value read out of a field or
//! an element is a closure cell, and so is the binding it initializes:
//! a destructured param `({ g }) => m(g)` is `let g = src.g`. Left out,
//! `m`'s fn-typed param kept the bare-pointer lane and called the
//! cell's header (EXIT 138).
//!
//! The answer is the set of `Ident` reads that resolve to such a
//! binding, not its name: `const inc = o.inc` holding a number in one
//! function must not make a top-level `function inc` passed elsewhere
//! read as a cell, which would keep it from being wrapped into its
//! forwarder at a retagged param.

use crate::ast::{Ast, Expr, ExprId, Param, Stmt};
use crate::ast_closure_param_tag::push_child_stmts;
use crate::ast_closure_param_tag::slot_walk::{Bound, Frame, Walk, declare};
use std::collections::{HashMap, HashSet};

/// Every `Ident` read of a binding whose initializer is (or chooses
/// among) member or index reads and whose annotation, if any, is a
/// fn type.
pub(crate) fn slot_bound_reads(ast: &Ast) -> HashSet<ExprId> {
    let mut bodies: Vec<(Option<&str>, &[Stmt], &[Param])> = vec![(None, &ast.stmts, &[])];
    let mut lifted: HashMap<&str, &[Param]> = (ast.exprs.iter())
        .filter_map(|e| match e {
            Expr::Closure { fn_name, .. } => Some((fn_name.as_str(), &[][..])),
            _ => None,
        })
        .collect();
    let mut padded: HashMap<&str, &[Param]> = HashMap::new();
    let mut stack: Vec<&Stmt> = ast.stmts.iter().collect();
    while let Some(s) = stack.pop() {
        if let Stmt::FnDecl {
            name, params, body, ..
        } = s
        {
            bodies.push((Some(name), body, params));
            if let Some(ps) = lifted.get_mut(name.as_str()) {
                *ps = params;
            }
            if ast.nested_fn_names.contains_key(name) {
                padded.insert(name, params);
            }
        }
        push_child_stmts(s, &mut stack);
    }
    // a nested declaration or lifted closure inherits the names in
    // scope where it sits; that is only known once its parent is walked
    let mut inherited: HashMap<String, Frame> = HashMap::new();
    loop {
        let mut w = Walk {
            ast,
            frames: Vec::new(),
            reads: HashSet::new(),
            inherited: HashMap::new(),
            lifted: &lifted,
            padded: &padded,
        };
        for (name, body, params) in &bodies {
            let seed = name.and_then(|n| inherited.get(n));
            w.frames = vec![seed.cloned().unwrap_or_default()];
            let ps = params.iter().map(|p| (p.name.clone(), param_bound(p)));
            w.frames.push(ps.collect());
            // a lifted closure's defaults are read where the closure
            // sits, a lifted nested declaration's where it is called
            if !name.is_some_and(|n| lifted.contains_key(n) || padded.contains_key(n)) {
                w.defaults(params);
            }
            let mut vars = Frame::new();
            hoisted_vars(ast, body, &mut vars);
            w.frames.last_mut().unwrap().extend(vars);
            w.block(body);
        }
        if w.inherited == inherited {
            return w.reads;
        }
        inherited = w.inherited;
    }
}

/// What a declaration with this annotation and initializer holds.
pub(crate) fn bound_of(ast: &Ast, type_ann: &Option<String>, init: ExprId) -> Bound {
    let ann = type_ann.as_deref();
    let slot_read = ann.is_none_or(crate::ast_collect_fn_closure_init::is_fn_slot_ann)
        && crate::ast_closure_param_tag_member::passthrough_leaves(ast, init)
            .iter()
            .any(|&l| matches!(ast.get_expr(l), Expr::Member { .. } | Expr::Index { .. }));
    if slot_read {
        Bound::SlotRead
    } else if ann.is_some_and(crate::ast::is_fn_arr_ann) {
        Bound::FnElems
    } else {
        Bound::Other
    }
}

fn param_bound(p: &Param) -> Bound {
    match p.type_ann.as_deref() {
        Some(a) if crate::ast::is_fn_arr_ann(a) => Bound::FnElems,
        _ => Bound::Other,
    }
}

/// A body's `var` bindings, hoisted to its own frame.
fn hoisted_vars(ast: &Ast, body: &[Stmt], out: &mut Frame) {
    let mut stack: Vec<&Stmt> = body.iter().collect();
    while let Some(s) = stack.pop() {
        match s {
            Stmt::FnDecl { .. } => continue,
            Stmt::LetDecl {
                name,
                init,
                type_ann,
                is_var: true,
                ..
            } => {
                declare(out, name, bound_of(ast, type_ann, *init));
            }
            _ => {}
        }
        push_child_stmts(s, &mut stack);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::lift_arrow_fns;
    use crate::lexer::tokenize;
    use crate::parser::parse;

    const PRE: &str = "type F = (x: number) => number;\n\
                       function m(f: F): number { return f(1); }\n";

    /// How many of the slot-bound reads name `n`.
    fn bound_reads_of(src: &str, n: &str) -> usize {
        let src = format!("{PRE}{src}");
        let toks = tokenize(&src).expect("lex");
        let mut ast = parse(&src, &toks).expect("parse");
        crate::ast::resolve_type_decls(&mut ast).expect("type decls");
        crate::ast::desugar_capturing_nested_fns(&mut ast);
        crate::ast::desugar_nested_fns(&mut ast);
        lift_arrow_fns(&mut ast);
        let reads = slot_bound_reads(&ast);
        let named = |e: &&ExprId| matches!(ast.get_expr(**e), Expr::Ident(i) if i == n);
        reads.iter().filter(named).count()
    }

    #[test]
    fn exported_binding_is_in_scope() {
        let src = "const p = { g: (x: number) => x + 1 };\nexport const g = p.g;\nm(g);\n";
        assert_eq!(bound_reads_of(src, "g"), 1);
        let src = "const p = { g: (x: number) => x + 1 };\nexport const { g } = p;\nm(g);\n";
        assert_eq!(bound_reads_of(src, "g"), 1);
    }

    #[test]
    fn param_default_reads_the_binding() {
        let decl =
            "function o({ g }: { g: F }) { function t(h: F = g) { return m(h); } return t(); }\n";
        assert_eq!(bound_reads_of(decl, "g"), 1);
        let arrow = "const o = ({ g }: { g: F }) => ((h: F = g) => m(h))();\n";
        assert_eq!(bound_reads_of(arrow, "g"), 1);
        // the pasted default reads the caller's `g`, a plain function
        let shadowed = "const src = { g: (x: number) => x + 1 };\n\
                        const g = src.g;\n\
                        function twice(x: number) { return x * 2; }\n\
                        function o() { const g = twice; function t(h: F = g) { return m(h); } return t(); }\n";
        assert_eq!(bound_reads_of(shadowed, "g"), 0);
    }

    #[test]
    fn lifted_closure_sees_its_capture_not_the_top_level() {
        let src = "const src = { g: (x: number) => x + 1 };\n\
                   const g = src.g;\n\
                   function twice(x: number) { return x * 2; }\n\
                   function run() { const g = twice; const t = () => m(g); return t(); }\n\
                   m(g);\n";
        // only the top-level `m(g)`; the closure's `g` is `twice`
        assert_eq!(bound_reads_of(src, "g"), 1);
        let caught = "const src = { g: (x: number) => x + 1 };\n\
                      const g = src.g;\n\
                      const t = () => m(g);\n";
        assert_eq!(bound_reads_of(caught, "g"), 1);
    }

    #[test]
    fn for_of_over_fn_values_binds_a_cell() {
        let arr =
            "function each(fs: F[]) { let s = 0; for (const g of fs) s += m(g); return s; }\n";
        assert_eq!(bound_reads_of(arr, "g"), 1);
        let caught =
            "function each(fs: Array<F>) { for (const g of fs) { const t = () => m(g); t(); } }\n";
        assert_eq!(bound_reads_of(caught, "g"), 1);
        let nums =
            "function sum(xs: number[]) { let s = 0; for (const g of xs) s += g; return s; }\n";
        assert_eq!(bound_reads_of(nums, "g"), 0);
        let shadowed = "function each(fs: F[]) { { const fs = [1]; for (const g of fs) m(g); } }\n";
        assert_eq!(bound_reads_of(shadowed, "g"), 0);
    }
}
