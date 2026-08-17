//! Knife D — the annotation-string face of a class rename.
//!
//! A class name appears in type positions as a bare word inside flat
//! ann strings (`let x: C`, `(p: C[]) => …`, `as C`, `new Box<C>()`),
//! which the census's Ident rewrite never sees. This walk applies the
//! same word-boundary substitution monomorphization uses to every ann
//! carrier in the lib's statements and arena slice.
//!
//! Shadowing: the census DECLINES the class mangle when any lib decl
//! declares a type parameter spelled like the class
//! (`type_param_shadows`), so by the time this walk runs no ann can
//! mean anything but the class. The per-decl skip below keeps the
//! same posture defensively anyway — a skipped signature under a
//! shadowing type param is exactly what the source meant.

use crate::ast::{Ast, Expr, Stmt};
use crate::ssa_lower_generics_monomorph::substitute_in_ann;

pub(super) fn rewrite_anns(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    old: &str,
    new: &str,
) {
    let subst = [(old.to_string(), new.to_string())];
    for s in lib_section.iter_mut() {
        stmt_anns(s, old, &subst);
    }
    for e in ast.exprs[lib_expr_offset..].iter_mut() {
        match e {
            Expr::As { ty_ann, .. } => *ty_ann = substitute_in_ann(ty_ann, &subst),
            Expr::New { type_args, .. } => {
                for t in type_args.iter_mut() {
                    *t = substitute_in_ann(t, &subst);
                }
            }
            // An arrow's body statements live in the arena, out of the
            // statement walk's reach — anns are plain stmt fields, so
            // the walk descends here without touching the arena again.
            Expr::ArrowFn {
                params,
                return_type,
                body,
            } => {
                for p in params.iter_mut() {
                    sub_opt(&mut p.type_ann, &subst);
                }
                sub_opt(return_type, &subst);
                for s in body.iter_mut() {
                    stmt_anns(s, old, &subst);
                }
            }
            _ => {}
        }
    }
}

fn sub_opt(ann: &mut Option<String>, subst: &[(String, String)]) {
    if let Some(a) = ann {
        *a = substitute_in_ann(a, subst);
    }
}

fn sub_params(params: &mut [crate::ast::Param], subst: &[(String, String)]) {
    for p in params.iter_mut() {
        sub_opt(&mut p.type_ann, subst);
    }
}

fn stmt_anns(s: &mut Stmt, old: &str, subst: &[(String, String)]) {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => stmt_anns(inner, old, subst),
        Stmt::LetDecl { type_ann, .. }
        | Stmt::UsingDecl { type_ann, .. }
        | Stmt::YieldInto { type_ann, .. } => sub_opt(type_ann, subst),
        Stmt::FnDecl {
            type_params,
            params,
            return_type,
            body,
            ..
        } => {
            if type_params.iter().any(|t| t == old) {
                return;
            }
            sub_params(params, subst);
            sub_opt(return_type, subst);
            for s in body.iter_mut() {
                stmt_anns(s, old, subst);
            }
        }
        Stmt::TypeDecl {
            type_params,
            fields,
            ..
        } => {
            if type_params.iter().any(|t| t == old) {
                return;
            }
            for (_, ty) in fields.iter_mut() {
                *ty = substitute_in_ann(ty, subst);
            }
        }
        Stmt::ClassDecl {
            type_params,
            fields,
            ctor,
            methods,
            static_methods,
            static_init,
            ..
        } => {
            if type_params.iter().any(|t| t == old) {
                return;
            }
            for (_, ty) in fields.iter_mut() {
                *ty = substitute_in_ann(ty, subst);
            }
            if let Some(c) = ctor {
                sub_params(&mut c.params, subst);
                for s in c.body.iter_mut() {
                    stmt_anns(s, old, subst);
                }
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                if m.type_params.iter().any(|t| t == old) {
                    continue;
                }
                sub_params(&mut m.params, subst);
                sub_opt(&mut m.return_type, subst);
                for s in m.body.iter_mut() {
                    stmt_anns(s, old, subst);
                }
            }
            for si in static_init.iter_mut() {
                match si {
                    crate::ast::StaticInit::Field(f) => {
                        f.type_ann = substitute_in_ann(&f.type_ann, subst);
                    }
                    crate::ast::StaticInit::Block(v) => {
                        for s in v.iter_mut() {
                            stmt_anns(s, old, subst);
                        }
                    }
                }
            }
        }
        Stmt::ForOf {
            var_type_ann, body, ..
        } => {
            sub_opt(var_type_ann, subst);
            stmt_anns(body, old, subst);
        }
        Stmt::ForOfSplitIter { body, .. } => stmt_anns(body, old, subst),
        Stmt::Try {
            catch_type,
            body,
            catch_body,
            finally_body,
            ..
        } => {
            sub_opt(catch_type, subst);
            for s in body
                .iter_mut()
                .chain(catch_body.iter_mut())
                .chain(finally_body.iter_mut().flatten())
            {
                stmt_anns(s, old, subst);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_anns(then_branch, old, subst);
            if let Some(eb) = else_branch {
                stmt_anns(eb, old, subst);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            stmt_anns(body, old, subst)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                stmt_anns(i, old, subst);
            }
            stmt_anns(body, old, subst);
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            for s in v.iter_mut() {
                stmt_anns(s, old, subst);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                for s in c.body.iter_mut() {
                    stmt_anns(s, old, subst);
                }
            }
            if let Some(d) = default {
                for s in d.iter_mut() {
                    stmt_anns(s, old, subst);
                }
            }
        }
        _ => {}
    }
}
