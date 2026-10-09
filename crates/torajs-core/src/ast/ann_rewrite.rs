//! The annotation-string face of a type-name rewrite.
//!
//! A type name appears in type positions as a bare word inside flat
//! ann strings (`let x: C`, `(p: C[]) => …`, `as C`, `new Box<C>()`),
//! which an Ident rewrite never sees. This walk hands every ann
//! carrier in a statement section and an arena slice to an
//! [`AnnRewrite`]. Its callers are the module deconflict's class
//! rename (Knife D: a lib's statements and arena slice, the word
//! substitution monomorphization uses) and the scalar type alias
//! inline (the whole program, a type-grammar-aware substitution).
//!
//! Shadowing: both callers DECLINE the rewrite when some decl declares
//! a type parameter spelled like the name, so by the time this walk
//! runs no ann can mean anything but the one type. The per-decl
//! `skips` keeps the same posture defensively anyway — a skipped
//! signature under a shadowing type param is exactly what the source
//! meant.

use crate::ast::{Ast, Expr, Stmt};

/// One annotation rewrite: which decls to leave alone, and what an
/// annotation becomes (`None` = unchanged).
pub(crate) trait AnnRewrite {
    /// A decl declaring these type parameters keeps its signatures.
    fn skips(&self, type_params: &[String]) -> bool;
    fn rewrite(&self, ann: &str) -> Option<String>;
}

pub(crate) fn rewrite_anns(
    ast: &mut Ast,
    lib_section: &mut [Stmt],
    lib_expr_offset: usize,
    rw: &dyn AnnRewrite,
) {
    for s in lib_section.iter_mut() {
        stmt_anns(s, rw);
    }
    for e in ast.exprs[lib_expr_offset..].iter_mut() {
        match e {
            Expr::As { ty_ann, .. } => sub(ty_ann, rw),
            Expr::New { type_args, .. } => {
                for t in type_args.iter_mut() {
                    sub(t, rw);
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
                sub_params(params, rw);
                sub_opt(return_type, rw);
                for s in body.iter_mut() {
                    stmt_anns(s, rw);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn sub(ann: &mut String, rw: &dyn AnnRewrite) {
    if let Some(a) = rw.rewrite(ann) {
        *ann = a;
    }
}

fn sub_opt(ann: &mut Option<String>, rw: &dyn AnnRewrite) {
    if let Some(a) = ann {
        sub(a, rw);
    }
}

fn sub_params(params: &mut [crate::ast::Param], rw: &dyn AnnRewrite) {
    for p in params.iter_mut() {
        sub_opt(&mut p.type_ann, rw);
    }
}

fn stmt_anns(s: &mut Stmt, rw: &dyn AnnRewrite) {
    match s {
        Stmt::ExportDecl {
            inner: Some(inner), ..
        } => stmt_anns(inner, rw),
        Stmt::LetDecl { type_ann, .. }
        | Stmt::UsingDecl { type_ann, .. }
        | Stmt::YieldInto { type_ann, .. } => sub_opt(type_ann, rw),
        Stmt::FnDecl {
            type_params,
            params,
            return_type,
            body,
            ..
        } => {
            if rw.skips(type_params) {
                return;
            }
            sub_params(params, rw);
            sub_opt(return_type, rw);
            for s in body.iter_mut() {
                stmt_anns(s, rw);
            }
        }
        Stmt::TypeDecl {
            type_params,
            fields,
            ..
        } => {
            if rw.skips(type_params) {
                return;
            }
            for (_, ty) in fields.iter_mut() {
                sub(ty, rw);
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
            if rw.skips(type_params) {
                return;
            }
            for (_, ty) in fields.iter_mut() {
                sub(ty, rw);
            }
            if let Some(c) = ctor {
                sub_params(&mut c.params, rw);
                for s in c.body.iter_mut() {
                    stmt_anns(s, rw);
                }
            }
            for m in methods.iter_mut().chain(static_methods.iter_mut()) {
                if rw.skips(&m.type_params) {
                    continue;
                }
                sub_params(&mut m.params, rw);
                sub_opt(&mut m.return_type, rw);
                for s in m.body.iter_mut() {
                    stmt_anns(s, rw);
                }
            }
            for si in static_init.iter_mut() {
                match si {
                    crate::ast::StaticInit::Field(f) => {
                        sub(&mut f.type_ann, rw);
                    }
                    crate::ast::StaticInit::Block(v) => {
                        for s in v.iter_mut() {
                            stmt_anns(s, rw);
                        }
                    }
                }
            }
        }
        Stmt::ForOf {
            var_type_ann, body, ..
        } => {
            sub_opt(var_type_ann, rw);
            stmt_anns(body, rw);
        }
        Stmt::ForOfSplitIter { body, .. } => stmt_anns(body, rw),
        Stmt::Try {
            catch_type,
            body,
            catch_body,
            finally_body,
            ..
        } => {
            sub_opt(catch_type, rw);
            for s in body
                .iter_mut()
                .chain(catch_body.iter_mut())
                .chain(finally_body.iter_mut().flatten())
            {
                stmt_anns(s, rw);
            }
        }
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => {
            stmt_anns(then_branch, rw);
            if let Some(eb) = else_branch {
                stmt_anns(eb, rw);
            }
        }
        Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
            stmt_anns(body, rw)
        }
        Stmt::For { init, body, .. } => {
            if let Some(i) = init {
                stmt_anns(i, rw);
            }
            stmt_anns(body, rw);
        }
        Stmt::Block(v) | Stmt::Multi(v) => {
            for s in v.iter_mut() {
                stmt_anns(s, rw);
            }
        }
        Stmt::Switch { cases, default, .. } => {
            for c in cases.iter_mut() {
                for s in c.body.iter_mut() {
                    stmt_anns(s, rw);
                }
            }
            if let Some(d) = default {
                for s in d.iter_mut() {
                    stmt_anns(s, rw);
                }
            }
        }
        _ => {}
    }
}
