//! The yield type of a generator written without a return-type
//! annotation.
//!
//! `function* range(a: number, b: number) { let i = a; while (i < b)
//! { yield i; i = i + 1 } }` yields numbers only, and TypeScript types
//! it `Generator<number, void, unknown>`. Desugared as `Generator<any>`
//! instead, every step boxed its value and every consumer read it back
//! through the any world: the loop variable of a `for…of` over it is
//! `any`, and so is everything it flows into.
//!
//! The step's `value` slot carries both the yielded values and the
//! completion value (§27.5.1.2 — a `return v` is the last step's
//! value), so the type is the one annotation every `yield` operand and
//! every valued `return` operand agree on. It is read off the prepared
//! body, where every parameter and lifted local is a `this.<name>`
//! field whose declared type is what a read of it answers. Anything
//! short of a unanimous, nameable answer keeps `any`: an operand the
//! static sniff cannot name (`yield* src`, whose values arrive through
//! the iterator protocol, among them), two operands of different
//! types, an annotation that mentions a type parameter — the
//! generator's own or one of an enclosing function or class, which the
//! lifted generator no longer sees (`function wrap<T>(x: T) { function*
//! g(y: T) { yield y } }`).
//!
//! Only the value channel is inferred. What `next(v)` sends in stays
//! `any` (TypeScript's `TNext` is `unknown` here), so `it.next("a")`
//! keeps compiling against a generator that yields numbers.

use crate::ast::desugar_with::walk;
use crate::ast::{Ast, Expr, Param, Stmt};
use std::collections::{HashMap, HashSet};

/// What the whole program decides for every unannotated generator,
/// read once before the generators are lifted and rewritten.
pub(crate) struct YieldInfer {
    on: bool,
    type_params: HashSet<String>,
}

impl YieldInfer {
    pub(crate) fn of_program(ast: &Ast) -> Self {
        let mut type_params: HashSet<String> =
            ast.class_type_params.values().flatten().cloned().collect();
        for s in &ast.stmts {
            collect_type_params(s, &mut type_params);
        }
        for e in &ast.exprs {
            if let Expr::ArrowFn { body, .. } = e {
                for s in body {
                    collect_type_params(s, &mut type_params);
                }
            }
        }
        Self {
            on: !program_names_return(ast),
            type_params,
        }
    }

    /// The step value type of an unannotated generator: the inferred
    /// one when the program allows it and `sync`, else `any`.
    pub(crate) fn yield_type(
        &self,
        ast: &Ast,
        sync: bool,
        (body, params, lifted_locals): (&[Stmt], &[Param], &[(String, String)]),
        fn_sigs: &HashMap<String, String>,
    ) -> String {
        (self.on && sync)
            .then(|| infer_yield_ann(ast, body, params, lifted_locals, fn_sigs))
            .flatten()
            .filter(|a| !self.names_type_param(a))
            .unwrap_or_else(|| "any".into())
    }

    fn names_type_param(&self, ann: &str) -> bool {
        ann.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
            .any(|w| self.type_params.contains(w))
    }
}

/// Every type parameter name a function, method, class or type alias
/// of the program declares, at any depth.
fn collect_type_params(s: &Stmt, out: &mut HashSet<String>) {
    match s {
        Stmt::FnDecl {
            type_params, body, ..
        } => {
            out.extend(type_params.iter().cloned());
            for c in body {
                collect_type_params(c, out);
            }
        }
        Stmt::TypeDecl { type_params, .. } => out.extend(type_params.iter().cloned()),
        Stmt::ClassDecl {
            type_params,
            ctor,
            methods,
            static_methods,
            ..
        } => {
            out.extend(type_params.iter().cloned());
            for c in ctor.iter().flat_map(|c| &c.body) {
                collect_type_params(c, out);
            }
            for m in methods.iter().chain(static_methods) {
                out.extend(m.type_params.iter().cloned());
                for c in &m.body {
                    collect_type_params(c, out);
                }
            }
        }
        _ => {
            for c in walk::stmt_children_ref(s) {
                collect_type_params(c, out);
            }
        }
    }
}

fn infer_yield_ann(
    ast: &Ast,
    body: &[Stmt],
    params: &[Param],
    lifted_locals: &[(String, String)],
    fn_sigs: &HashMap<String, String>,
) -> Option<String> {
    let mut binds: HashMap<String, String> = HashMap::new();
    for p in params {
        if let Some(ann) = &p.type_ann {
            binds.insert(format!("this.{}", p.name), ann.clone());
        }
    }
    for (name, ann) in lifted_locals {
        binds.insert(format!("this.{name}"), ann.clone());
    }
    let mut operands = Vec::new();
    for s in body {
        collect_step_operands(s, &mut operands);
    }
    let mut acc: Option<String> = None;
    for eid in operands {
        let ann = crate::ast::infer_expr_ann_with(&ast.exprs, eid, &[], &binds, fn_sigs)?;
        match &acc {
            None => acc = Some(ann),
            Some(prev) if *prev == ann => {}
            Some(_) => return None,
        }
    }
    acc.filter(|a| a != "any" && !crate::ast::implicit_generics_infer::mentions_typevar(a))
}

/// Every `yield` operand and valued `return` operand of the body,
/// nested declarations excluded (their yields and returns are their
/// own).
fn collect_step_operands(s: &Stmt, out: &mut Vec<crate::ast::ExprId>) {
    match s {
        Stmt::Yield(e) | Stmt::Return(Some(e)) => out.push(*e),
        Stmt::YieldInto { value, .. } => out.push(*value),
        _ => {}
    }
    for c in walk::stmt_children_ref(s) {
        collect_step_operands(c, out);
    }
}

/// Does the program name a `return` property anywhere — `it.return(v)`,
/// `it?.return`, `it["return"]`, a `"return"` string a reflective read
/// could key with?
///
/// The step's `value` slot is also what `return(v)` completes with
/// (§27.5.1.3), so an inferred `number` slot would type `return`'s
/// parameter `number`: `it.return("x")` would stop compiling and a
/// bare `it.return()` would miss its argument, refusals of programs
/// TypeScript and the spec both accept. A program that never reaches
/// the method by name keeps the inference; the closes the runtime
/// performs itself (for-of, destructuring) call it with `undefined`
/// and read only `done`.
fn program_names_return(ast: &Ast) -> bool {
    ast.exprs.iter().any(|e| match e {
        Expr::Member { name, .. } | Expr::OptChain { name, .. } => name == "return",
        Expr::String(s) => s.as_str() == Some("return"),
        _ => false,
    })
}
