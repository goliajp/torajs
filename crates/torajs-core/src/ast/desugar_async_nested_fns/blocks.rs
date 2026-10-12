//! Function declarations nested in a block of an async function or a
//! generator body.
//!
//! They want the same routes as the body's own declarations, for the
//! same reason: the machine spreads the body over its states, and the
//! later lanes (`nested_fns_capture`, `nested_fns`) only see the pieces.
//! `function* m() { let x = 5; yield 0; { function h() { return x; } } }`
//! read `x` as an unknown identifier, and `{ yield q(); function q() {} }`
//! read `q` as one. Each block list routes like a body (`select`): a
//! declaration reading the frames around it becomes a block `let`
//! holding a function value, one reading nothing is lifted with its
//! references renamed inside the block only — §14.2.3 makes it a
//! binding of the block, initialized on block entry.
//!
//! Sloppy code adds Annex B §B.3.3's var binding: the block binding takes
//! a name of its own and `var <name> = <block binding>` is written where
//! the declaration sits (§B.3.3.1 step 3.a.ii), unless an enclosing
//! lexical declaration or a parameter already owns the name.

use std::collections::HashMap;

use super::lift::Lifts;
use super::rename::rename_in_owner;
use crate::ast::annexb_fn_var::annexb_applies_at;
use crate::ast::scope_chain::{for_init_names, list_names};
use crate::ast::{Ast, Expr, Stmt};

impl Lifts {
    /// Route the declarations of every block under `body`, then the
    /// body's own. `frames` is `None` for an async body; `owners` holds
    /// the parameters, names §B.3.3 leaves alone.
    pub(super) fn route(
        &mut self,
        ast: &mut Ast,
        body: &mut [Stmt],
        parent: &str,
        frames: Option<&[String]>,
        owners: Vec<String>,
    ) {
        Blocks::new(self, parent).rewrite_body(ast, body, frames, owners);
        super::rewrite_declarations(ast, body, self, parent, frames);
    }
}

struct Blocks<'a> {
    lifts: &'a mut Lifts,
    parent: &'a str,
    split: u32,
}

impl<'a> Blocks<'a> {
    fn new(lifts: &'a mut Lifts, parent: &'a str) -> Self {
        Self {
            lifts,
            parent,
            split: 0,
        }
    }

    fn rewrite_body(
        &mut self,
        ast: &mut Ast,
        body: &mut [Stmt],
        frames: Option<&[String]>,
        mut owners: Vec<String>,
    ) {
        lexical_owners(body, &mut owners);
        let frames = frames.map(<[String]>::to_vec);
        for s in body.iter_mut() {
            self.walk(ast, s, frames.as_ref(), &owners);
        }
    }

    fn walk(
        &mut self,
        ast: &mut Ast,
        s: &mut Stmt,
        frames: Option<&Vec<String>>,
        owners: &[String],
    ) {
        match s {
            Stmt::Block(list) => self.block(ast, list, frames, owners, None),
            Stmt::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.walk(ast, then_branch, frames, owners);
                if let Some(e) = else_branch {
                    self.walk(ast, e, frames, owners);
                }
            }
            Stmt::While { body, .. } | Stmt::DoWhile { body, .. } | Stmt::Labeled { body, .. } => {
                self.walk(ast, body, frames, owners)
            }
            Stmt::For { init, body, .. } => {
                let mut bound = Vec::new();
                for_init_names(init.as_deref(), &mut bound);
                self.walk_bound(ast, body, frames, owners, bound);
            }
            Stmt::ForOf { var_name, body, .. } | Stmt::ForOfSplitIter { var_name, body, .. } => {
                let bound = vec![var_name.clone()];
                self.walk_bound(ast, body, frames, owners, bound);
            }
            Stmt::Try {
                body,
                catch_param,
                catch_body,
                finally_body,
                ..
            } => {
                self.block(ast, body, frames, owners, None);
                self.block(ast, catch_body, frames, owners, catch_param.as_deref());
                if let Some(f) = finally_body {
                    self.block(ast, f, frames, owners, None);
                }
            }
            Stmt::Multi(v) => {
                for c in v.iter_mut() {
                    self.walk(ast, c, frames, owners);
                }
            }
            _ => {}
        }
    }

    /// A loop body under the names its head declares.
    fn walk_bound(
        &mut self,
        ast: &mut Ast,
        body: &mut Stmt,
        frames: Option<&Vec<String>>,
        owners: &[String],
        bound: Vec<String>,
    ) {
        let inner = frames.map(|f| [f.as_slice(), &bound].concat());
        let owners = [owners, &bound].concat();
        self.walk(ast, body, inner.as_ref(), &owners);
    }

    fn block(
        &mut self,
        ast: &mut Ast,
        list: &mut Vec<Stmt>,
        frames: Option<&Vec<String>>,
        owners: &[String],
        catch_param: Option<&str>,
    ) {
        let mut bound: Vec<String> = catch_param.map(str::to_string).into_iter().collect();
        list_names(list, &mut bound);
        for s in list.iter() {
            if let Stmt::YieldInto { var, .. } = s {
                bound.push(var.clone());
            }
        }
        let inner = frames.map(|f| [f.as_slice(), &bound].concat());
        let mut inner_owners = owners.to_vec();
        list_names(list, &mut inner_owners);
        for s in list.iter_mut() {
            self.walk(ast, s, inner.as_ref(), &inner_owners);
        }
        if !list.iter().any(|s| matches!(s, Stmt::FnDecl { .. })) {
            return;
        }
        self.split_annexb(ast, list, owners);
        super::rewrite_declarations(ast, list, self.lifts, self.parent, inner.as_deref());
    }

    /// §B.3.3 in sloppy code: rename the block binding and write the var
    /// one where the declaration sits.
    fn split_annexb(&mut self, ast: &mut Ast, list: &mut Vec<Stmt>, owners: &[String]) {
        let mut i = 0;
        while i < list.len() {
            let Stmt::FnDecl {
                name,
                is_generator: false,
                span,
                ..
            } = &list[i]
            else {
                i += 1;
                continue;
            };
            let (name, span) = (name.clone(), *span);
            if owners.contains(&name)
                || ast.async_fns.contains(&name)
                || !annexb_applies_at(ast, &name, span)
            {
                i += 1;
                continue;
            }
            let block = format!("__blkfn_{name}_g{}", self.split);
            self.split += 1;
            ast.renamed_binding_display
                .insert(block.clone(), name.clone());
            let map = HashMap::from([(name.clone(), block.clone())]);
            rename_in_owner(ast, list, &map);
            if let Stmt::FnDecl { name: n, .. } = &mut list[i] {
                *n = block.clone();
            }
            let init = ast.add_expr(Expr::Ident(block));
            ast.set_expr_span(init, span);
            list.insert(
                i + 1,
                Stmt::LetDecl {
                    mutable: true,
                    name,
                    type_ann: None,
                    init,
                    is_var: true,
                },
            );
            i += 2;
        }
    }
}

/// The lexical declarations of a function body's own list, which a var
/// of the same name would collide with. Its function declarations are
/// var-like there and do not.
fn lexical_owners(body: &[Stmt], out: &mut Vec<String>) {
    let mut all = Vec::new();
    list_names(body, &mut all);
    let fns: Vec<&str> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::FnDecl { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    out.extend(all.into_iter().filter(|n| !fns.contains(&n.as_str())));
}

/// Does `body`, or any block under it, declare a function?
pub(super) fn declares_fn(body: &[Stmt]) -> bool {
    body.iter()
        .any(|s| matches!(s, Stmt::FnDecl { .. }) || stmt_nests(s))
}

fn stmt_nests(s: &Stmt) -> bool {
    match s {
        Stmt::Block(v) => v
            .iter()
            .any(|c| matches!(c, Stmt::FnDecl { .. }) || stmt_nests(c)),
        Stmt::Multi(v) => v.iter().any(stmt_nests),
        Stmt::If {
            then_branch,
            else_branch,
            ..
        } => stmt_nests(then_branch) || else_branch.as_deref().is_some_and(stmt_nests),
        Stmt::While { body, .. }
        | Stmt::DoWhile { body, .. }
        | Stmt::Labeled { body, .. }
        | Stmt::For { body, .. }
        | Stmt::ForOf { body, .. }
        | Stmt::ForOfSplitIter { body, .. } => stmt_nests(body),
        Stmt::Try {
            body,
            catch_body,
            finally_body,
            ..
        } => [body, catch_body]
            .into_iter()
            .chain(finally_body.as_ref())
            .any(|l| {
                l.iter()
                    .any(|c| matches!(c, Stmt::FnDecl { .. }) || stmt_nests(c))
            }),
        _ => false,
    }
}
