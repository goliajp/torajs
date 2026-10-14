//! A `type` or `interface` declared inside a function or a block is
//! scoped to that block (TS spec §3.10 / §7: type declarations are
//! block-scoped like `let`), and shadows a declaration of the same
//! name further out. Every reader downstream — the checker's type
//! tables, the alias inline, the lowering's alias registry — looks
//! type names up in one program-wide table built from the top-level
//! statements, so a nested declaration was never registered at all
//! (`unknown type`) and its name read as the outer declaration
//! (`type K = string` outside, `type K = number` inside: `const x: K
//! = 41` was a mismatch against `string`).
//!
//! This pass gives every nested type declaration a name of its own,
//! rewrites each annotation in its scope to that name, and moves the
//! declaration to the top level, where every table already looks. A
//! scope is a statement list (a function body, a block, a try or catch
//! body, the whole of a switch); a function's own signature sees the
//! declarations of its body. A type parameter shadows an outer
//! declaration of its name for the signature and body it belongs to,
//! and a nested declaration that names an enclosing type parameter
//! stays where it is: lifted out, the parameter would mean nothing.

use std::collections::{HashMap, HashSet};

use super::scalar_alias_spell::{inline_ann, type_words};
use super::{Ast, Param, Stmt};

/// The type names visible at a point: the nested declarations in scope
/// (source name to unique name) and the type parameters in scope.
#[derive(Clone, Default)]
struct Env {
    names: HashMap<String, String>,
    tparams: HashSet<String>,
}

impl Env {
    fn under_tparams(&self, tparams: &[String]) -> Env {
        let mut e = self.clone();
        for t in tparams {
            e.names.remove(t);
            e.tparams.insert(t.clone());
        }
        e
    }

    fn sub(&self, ann: &mut String) {
        if self.names.is_empty() {
            return;
        }
        if let Some(a) = inline_ann(ann, &self.names) {
            *ann = a;
        }
    }

    fn sub_opt(&self, ann: &mut Option<String>) {
        if let Some(a) = ann {
            self.sub(a);
        }
    }
}

/// The type-declaration prelude every pipeline runs before anything
/// else reads a type name: enums become their alias and object, nested
/// type declarations move to the top level under names of their own,
/// and scalar and function-type aliases are inlined.
pub fn resolve_type_decls(ast: &mut Ast) -> Result<(), String> {
    super::desugar_enums(ast)?;
    hoist_scoped_type_decls(ast);
    super::scalar_alias_inline::inline_scalar_aliases(ast);
    Ok(())
}

fn hoist_scoped_type_decls(ast: &mut Ast) {
    let mut stmts = std::mem::take(&mut ast.stmts);
    let mut h = Hoist {
        taken: top_level_type_names(&stmts),
        hoisted: Vec::new(),
    };
    h.list(ast, &mut stmts, &Env::default());
    stmts.append(&mut h.hoisted);
    ast.stmts = stmts;
}

fn top_level_type_names(stmts: &[Stmt]) -> HashSet<String> {
    stmts
        .iter()
        .filter_map(|s| match s {
            Stmt::TypeDecl { name, .. } | Stmt::ClassDecl { name, .. } => Some(name.clone()),
            _ => None,
        })
        .collect()
}

struct Hoist {
    /// names a unique name must not collide with
    taken: HashSet<String>,
    hoisted: Vec<Stmt>,
}

impl Hoist {
    fn fresh(&mut self, name: &str) -> String {
        let mut n = 1usize;
        loop {
            let cand = format!("{name}$$scope{n}");
            if self.taken.insert(cand.clone()) {
                return cand;
            }
            n += 1;
        }
    }

    /// Name the type declarations directly in `list`, lift them out,
    /// and answer the environment the rest of the scope sees.
    fn declare(&mut self, ast: &mut Ast, lists: &mut [&mut Vec<Stmt>], env: &Env) -> Env {
        let mut inner = env.clone();
        for list in lists.iter() {
            for s in list.iter() {
                let Stmt::TypeDecl {
                    name,
                    type_params,
                    fields,
                } = s
                else {
                    continue;
                };
                // a declaration that stays in place still shadows an
                // outer one of its name for the rest of this scope
                if names_enclosing_tparam(fields, type_params, &env.tparams) {
                    inner.names.remove(name);
                } else {
                    let fresh = self.fresh(name);
                    inner.names.insert(name.clone(), fresh);
                }
            }
        }
        for list in lists.iter_mut() {
            let mut kept = Vec::with_capacity(list.len());
            for s in list.drain(..) {
                match s {
                    Stmt::TypeDecl {
                        name,
                        type_params,
                        mut fields,
                    } if inner
                        .names
                        .get(&name)
                        .is_some_and(|n| env.names.get(&name) != Some(n)) =>
                    {
                        let fresh = inner.names[&name].clone();
                        let own = inner.under_tparams(&type_params);
                        for (_, ty) in fields.iter_mut() {
                            own.sub(ty);
                        }
                        rekey_side_tables(ast, &name, &fresh, &own);
                        self.hoisted.push(Stmt::TypeDecl {
                            name: fresh,
                            type_params,
                            fields,
                        });
                    }
                    s => kept.push(s),
                }
            }
            **list = kept;
        }
        inner
    }

    fn scope(&mut self, ast: &mut Ast, list: &mut Vec<Stmt>, env: &Env) {
        let env = self.declare(ast, &mut [&mut *list], env);
        self.list(ast, list, &env);
    }

    fn list(&mut self, ast: &mut Ast, list: &mut [Stmt], env: &Env) {
        for s in list.iter_mut() {
            self.stmt(ast, s, env);
        }
    }

    /// A function-shaped body: its type parameters, then the
    /// declarations of its body, are in scope for its signature.
    fn function(
        &mut self,
        ast: &mut Ast,
        tparams: &[String],
        params: &mut [Param],
        ret: &mut Option<String>,
        body: &mut Vec<Stmt>,
        env: &Env,
    ) {
        let env = env.under_tparams(tparams);
        let env = self.declare(ast, &mut [&mut *body], &env);
        self.params(ast, params, &env);
        env.sub_opt(ret);
        self.list(ast, body, &env);
    }

    fn params(&mut self, ast: &mut Ast, params: &mut [Param], env: &Env) {
        for p in params.iter_mut() {
            env.sub_opt(&mut p.type_ann);
            if let Some(d) = p.default {
                self.expr(ast, d, env);
            }
        }
    }
}

/// Whether a declaration's fields name a type parameter of an
/// enclosing declaration (its own parameters excepted).
fn names_enclosing_tparam(
    fields: &[(super::PropKey, String)],
    own: &[String],
    enclosing: &HashSet<String>,
) -> bool {
    !enclosing.is_empty()
        && fields.iter().any(|(_, ty)| {
            type_words(ty).any(|w| enclosing.contains(w) && !own.iter().any(|o| o == w))
        })
}

/// The parser's per-declaration records follow the declaration to its
/// new name. The literal-type record exists only for a name declared
/// once, so a record under the old name is this declaration's.
fn rekey_side_tables(ast: &mut Ast, old: &str, new: &str, env: &Env) {
    if let Some(Some(mut precise)) = ast.literal_type_decls.remove(old) {
        for (_, ty) in precise.iter_mut() {
            env.sub(ty);
        }
        ast.literal_type_decls
            .insert(new.to_string(), Some(precise));
    }
    let rekey = |set: &std::collections::HashSet<(String, super::PropKey)>| {
        set.iter()
            .filter(|(owner, _)| owner == old)
            .map(|(_, k)| (new.to_string(), k.clone()))
            .collect::<Vec<_>>()
    };
    let optional = rekey(&ast.optional_members.all);
    ast.optional_members.all.extend(optional);
    let plain = rekey(&ast.optional_members.plain);
    ast.optional_members.plain.extend(plain);
}

mod walk;

#[cfg(test)]
mod tests;
