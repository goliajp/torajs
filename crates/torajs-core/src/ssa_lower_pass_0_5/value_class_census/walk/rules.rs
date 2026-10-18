//! The whitelist itself: which expression shapes may hold an instance,
//! in which places ([`Walk::allowed`]), and the place each child of an
//! expression sits in ([`Walk::children`]).

use crate::ast::{Expr, ExprId};
use crate::check::Type;

use super::super::program::Cat;
use super::fns::call_site_ok;
use super::{Ctx, Role, Walk, role_of};

impl<'p, 'a> Walk<'p, 'a> {
    pub(super) fn allowed(
        &mut self,
        e: ExprId,
        x: &'a Expr,
        cat: Cat,
        ctx: &Ctx<'a>,
    ) -> Result<(), String> {
        let h = self.h;
        let ast = self.prog.ast;
        if let (Cat::Elem, Expr::Index { obj, .. }) = (cat, x)
            && !self.rooted(*obj)
        {
            return Err("an element read off a temporary array".into());
        }
        if let (Cat::Elem, Expr::Ident(n)) = (cat, x)
            && self.view_of(n).is_none()
        {
            return Err(format!("a read of `{n}` that no view in scope binds"));
        }
        let ok = match cat {
            Cat::Bad => false,
            Cat::Elem => match x {
                Expr::Index { obj, index } => {
                    self.cat(*obj) == Some(Cat::Arr)
                        && self.prog.ty(*index) == Some(&Type::Number)
                        && match ctx {
                            Ctx::MemberObj { name, role } => self.field_read(name, *role),
                            Ctx::Let { mutable, ann } => {
                                !mutable && ann.is_none_or(|a| a.trim() == h.class)
                            }
                            Ctx::Discard => true,
                            _ => false,
                        }
                }
                Expr::Ident(n) => {
                    if let Some(view) = self.view_of(n) {
                        *self.ident_reads.entry(view).or_default() += 1;
                    }
                    matches!(ctx, Ctx::MemberObj { name, role } if self.field_read(name, *role))
                }
                Expr::Call { callee, args } => {
                    matches!(ast.get_expr(*callee), Expr::Ident(n) if n.strip_prefix("__new_") == Some(h.class))
                        && self.construct_args_ok(args)
                        && matches!(ctx, Ctx::PushArg | Ctx::Discard)
                }
                Expr::New {
                    class_name, args, ..
                } => {
                    class_name == h.class
                        && self.construct_args_ok(args)
                        && matches!(ctx, Ctx::PushArg | Ctx::Discard)
                }
                // an object literal is a struct, not an instance: the
                // in-place push builds only from a construction
                _ => false,
            },
            Cat::Arr | Cat::Boxed(_) => self.shape_ok(x, cat) && self.place_ok(cat, ctx),
        };
        if ok {
            Ok(())
        } else {
            let ty = self.prog.ty(e);
            let under = match self.parent {
                Some(p) => {
                    let p = format!("{:?}", ast.get_expr(p));
                    p.split(['(', ' ']).next().unwrap_or_default().to_string()
                }
                None => "a statement".into(),
            };
            Err(format!(
                "an expression typed {ty:?} in an unknown place ({ctx:?} under {under}): {x:?}"
            ))
        }
    }

    /// An array that outlives the statement reading it: a binding, or
    /// a container field reached from one. A temporary array (a call's
    /// result, `new C(..).f`) is released right after its element is
    /// read, when the element takes a stake of its own; an element
    /// held in place would be left pointing into freed memory.
    fn rooted(&self, e: ExprId) -> bool {
        match self.prog.ast.get_expr(e) {
            Expr::Ident(_) | Expr::This => true,
            Expr::Member { obj, .. } => {
                matches!(self.cat(*obj), Some(Cat::Boxed(_))) && self.rooted(*obj)
            }
            _ => false,
        }
    }

    fn field_read(&self, name: &str, role: Role) -> bool {
        role == Role::Read && self.h.fields.contains(&name)
    }

    fn construct_args_ok(&self, args: &[ExprId]) -> bool {
        args.len() == self.h.arity
            && args
                .iter()
                .all(|a| !matches!(self.prog.ast.get_expr(*a), Expr::Spread { .. }))
    }

    /// How an array or a container may be produced.
    fn shape_ok(&self, x: &Expr, cat: Cat) -> bool {
        match x {
            Expr::Ident(_) => true,
            Expr::Assign { value, .. } => self.cat(*value) == Some(cat),
            Expr::Member { obj, name } => {
                matches!(self.cat(*obj), Some(Cat::Boxed(i)) if self.h.containers[i].1.contains(&name.as_str()))
            }
            Expr::Call { callee, .. } => matches!(self.prog.ast.get_expr(*callee),
                Expr::Ident(n) if self.watched.get(n.as_str()).is_some_and(|w| w.ret == Some(cat))),
            Expr::New { class_name, .. } => {
                matches!(cat, Cat::Boxed(i) if self.h.containers[i].0 == class_name)
            }
            Expr::Array(xs) => xs.is_empty() && cat == Cat::Arr,
            _ => false,
        }
    }

    /// Where an array or a container may go.
    fn place_ok(&self, cat: Cat, ctx: &Ctx<'_>) -> bool {
        match *ctx {
            Ctx::Discard | Ctx::AssignTarget => true,
            Ctx::Let { ann, .. } => ann.is_none_or(|a| self.h.ann_cat(a) == Some(cat)),
            Ctx::Return => self.cur_ret == Some(cat),
            Ctx::Arg { want } => want == Some(cat),
            Ctx::AssignValue { assign } => self.cat(assign) == Some(cat),
            Ctx::IndexObj { index, role } => {
                cat == Cat::Arr && role == Role::Read && self.prog.ty(index) == Some(&Type::Number)
            }
            Ctx::PushRecv => cat == Cat::Arr,
            Ctx::MemberObj { name, role } => match cat {
                Cat::Arr => name == "length" && role == Role::Read,
                Cat::Boxed(i) => {
                    matches!(role, Role::Read | Role::Write)
                        && self.h.containers[i].1.contains(&name)
                }
                _ => false,
            },
            _ => false,
        }
    }

    pub(super) fn children(&mut self, e: ExprId, x: &'a Expr, ctx: &Ctx<'a>) {
        let ast = self.prog.ast;
        let role = role_of(ctx);
        match x {
            Expr::Ident(n) if !self.exempt => {
                let c = self.h.class;
                if n == c
                    || n.strip_prefix("__cm_")
                        .and_then(|r| r.strip_suffix("__ctor"))
                        == Some(c)
                {
                    self.fail(format!("`{n}` read as a value"));
                }
                let held = self.binding(n);
                if held.is_some() && !matches!(ctx, Ctx::AssignTarget) && self.cat(e) != held {
                    self.fail(format!("a read of `{n}` typed {:?}", self.prog.ty(e)));
                }
            }
            Expr::Member { obj, name } => {
                if let Some(Cat::Boxed(i)) = self.cat(*obj) {
                    let want = self.h.field_cat(i, name);
                    self.read_as(e, want, ctx, || format!("a read of `.{name}`"));
                }
                self.expr(*obj, Ctx::MemberObj { name, role })
            }
            Expr::OptChain { obj, name } => self.expr(
                *obj,
                Ctx::MemberObj {
                    name,
                    role: Role::Other,
                },
            ),
            Expr::Index { obj, index } => {
                if self.cat(*obj) == Some(Cat::Arr) && self.prog.ty(*index) == Some(&Type::Number) {
                    self.read_as(e, Some(Cat::Elem), ctx, || "an element read".into());
                }
                self.expr(
                    *obj,
                    Ctx::IndexObj {
                        index: *index,
                        role,
                    },
                );
                self.expr(*index, Ctx::Other);
            }
            Expr::OptIndex { obj, index } => {
                self.expr(
                    *obj,
                    Ctx::IndexObj {
                        index: *index,
                        role: Role::Other,
                    },
                );
                self.expr(*index, Ctx::Other);
            }
            Expr::Call { callee, args } => self.call(e, *callee, args),
            Expr::New {
                class_name, args, ..
            } => {
                let factory = format!(
                    "__new_{}",
                    class_name.split('<').next().unwrap_or(class_name)
                );
                let want = self.h.ann_cat(class_name).filter(|c| *c != Cat::Bad);
                self.read_as(e, want, ctx, || format!("`new {class_name}`"));
                self.args(&factory, args);
            }
            Expr::Assign { target, value } => {
                let slot = match ast.get_expr(*target) {
                    Expr::Member { name, .. } => self.h.holding_fields.contains(name.as_str()),
                    Expr::Ident(n) => self.binding(n).is_some(),
                    _ => false,
                };
                if !self.exempt && slot && self.cat(e).is_none() {
                    self.fail("a store into a holding slot of another type".into());
                }
                self.expr(*target, Ctx::AssignTarget);
                self.expr(*value, Ctx::AssignValue { assign: e });
            }
            Expr::PostIncr { target, .. } => self.expr(*target, Ctx::Mutate),
            Expr::Delete { expr } => self.expr(*expr, Ctx::Mutate),
            Expr::InstanceOf { expr, rhs } => {
                self.expr(*expr, Ctx::Other);
                self.expr(*rhs, Ctx::Other);
            }
            Expr::OptCall { callee, args } | Expr::NewDynamic { callee, args } => {
                self.expr(*callee, Ctx::Other);
                args.iter()
                    .for_each(|a| self.expr(*a, Ctx::Arg { want: None }));
            }
            Expr::Super { args } => args
                .iter()
                .for_each(|a| self.expr(*a, Ctx::Arg { want: None })),
            Expr::Closure { fn_name, .. } => {
                if !self.exempt && self.watched.contains_key(fn_name.as_str()) {
                    self.fail(format!("`{fn_name}` taken as a value"));
                }
            }
            Expr::ArrowFn { params, body, .. } => {
                let saved = (self.cur_fn, self.cur_ret);
                (self.cur_fn, self.cur_ret) = (None, None);
                self.enter_scope(params, body);
                for p in params {
                    if let Some(d) = p.default {
                        self.expr(d, Ctx::Other);
                    }
                }
                self.stmts(body);
                self.leave_scope();
                (self.cur_fn, self.cur_ret) = saved;
            }
            _ => crate::ast::desugar_async_sm_walk::visit_expr_children(ast, e, &mut |c| {
                self.expr(c, Ctx::Other)
            }),
        }
    }

    /// A read the declarations say holds `want` must be typed so: a
    /// read the checker typed `any` would hand the instance on with no
    /// expression of the walk holding it.
    fn read_as(
        &mut self,
        e: ExprId,
        want: Option<Cat>,
        ctx: &Ctx<'a>,
        what: impl FnOnce() -> String,
    ) {
        if !self.exempt
            && want.is_some()
            && !matches!(ctx, Ctx::AssignTarget)
            && self.cat(e) != want
        {
            self.fail(format!("{} typed {:?}", what(), self.prog.ty(e)));
        }
    }

    fn call(&mut self, e: ExprId, callee: ExprId, args: &'a [ExprId]) {
        let ast = self.prog.ast;
        if let Expr::Ident(n) = ast.get_expr(callee) {
            let want = match n.strip_prefix("__new_") {
                Some(c) => self.h.ann_cat(c).filter(|c| *c != Cat::Bad),
                None => self.watched.get(n.as_str()).and_then(|w| w.ret),
            };
            self.read_as(e, want, &Ctx::Other, || format!("a call of `{n}`"));
        }
        match ast.get_expr(callee) {
            Expr::Member { obj, name } if name == "push" && self.cat(*obj) == Some(Cat::Arr) => {
                // a value of any other type pushed here would sit in a
                // slot read as an instance
                let arg_ok = matches!(args, [a] if self.cat(*a) == Some(Cat::Elem));
                self.visited[callee.0 as usize] = true;
                self.expr(*obj, if arg_ok { Ctx::PushRecv } else { Ctx::Other });
                args.iter().for_each(|a| self.expr(*a, Ctx::PushArg));
            }
            Expr::Member { .. } => {
                self.expr(callee, Ctx::Callee);
                args.iter()
                    .for_each(|a| self.expr(*a, Ctx::Arg { want: None }));
            }
            Expr::Ident(n)
                if self.prog.fns.contains_key(n.as_str())
                    && !self.prog.bound.contains(n.as_str()) =>
            {
                self.expr(callee, Ctx::Callee);
                self.args(n, args);
            }
            _ => {
                self.expr(callee, Ctx::Other);
                args.iter()
                    .for_each(|a| self.expr(*a, Ctx::Arg { want: None }));
            }
        }
    }

    /// The arguments of a direct call of `name`.
    fn args(&mut self, name: &str, args: &'a [ExprId]) {
        let wants: Vec<Option<Cat>> = match self.watched.get(name) {
            Some(w) if !self.exempt && !call_site_ok(self.prog, self.h, w, args) => {
                self.fail(format!(
                    "a call of `{name}` passes another value where it names the class"
                ));
                return;
            }
            Some(w) => w.params.clone(),
            None => Vec::new(),
        };
        for (i, a) in args.iter().enumerate() {
            let want = wants.get(i).copied().flatten();
            self.expr(*a, Ctx::Arg { want });
        }
    }
}
