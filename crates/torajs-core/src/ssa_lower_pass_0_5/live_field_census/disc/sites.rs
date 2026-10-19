//! Which construction sites a read under a fact can see, and what
//! each of them stores in the field read.

use crate::ast::{Expr, ExprId, Stmt};
use crate::ssa::id_hash::HashMap;
use crate::ssa_lower_pass_0_5::arr_head_census::ExprMask;

use super::super::judge::{State, Visiting};
use super::super::scope::Binder;
use super::super::walk::{ctor_prefix, derives_from};
use super::super::{Census, PrefixWrite, THIS};
use super::{Disc, Discs, Key, Twins, key};

impl<'a> Census<'a> {
    pub(super) fn twins(&self, class: &'a str) -> Twins<'a> {
        let mut t = Twins::default();
        let ctor = format!("__cm_{class}__ctor");
        let twin_ctor = format!("{ctor}$$");
        let twin_factory = format!("__new_{class}$$");
        for s in &self.ast.stmts {
            let Stmt::FnDecl {
                name, params, body, ..
            } = s
            else {
                continue;
            };
            if name.starts_with(&twin_ctor) {
                t.ctors += 1;
                let ws = ctor_prefix(self.ast, (name, class), params, body);
                t.writes.extend(ws.into_iter().map(|(_, w)| w));
            }
            // `let __this = {..}` handed straight to a constructor
            if name.starts_with(&twin_factory)
                && let Some(at) = (body.iter())
                    .position(|s| matches!(s, Stmt::LetDecl { name, .. } if name == THIS))
                && let Stmt::LetDecl { init, .. } = &body[at]
                && let Expr::ObjectLit { fields } = self.ast.get_expr(*init)
                && let Some(Stmt::Expr(call)) = body.get(at + 1)
                && let Expr::Call { callee, .. } = self.ast.get_expr(*call)
                && matches!(self.ast.get_expr(*callee), Expr::Ident(n) if n.starts_with(&ctor))
            {
                t.seeds.extend(fields.iter().map(|(_, v)| *v));
            }
        }
        t
    }

    /// For each read under a fact, the values the construction sites
    /// its facts have not ruled out store in the field read, all of
    /// which must be live; none when some site cannot be told.
    pub(in super::super) fn disc_plans(
        &self,
        discs: &mut Discs<'a>,
        construct_args: &HashMap<&'a str, Vec<&'a [ExprId]>>,
        fields_open: bool,
    ) {
        let plans = (discs.reads.iter())
            .map(|(e, d)| (*e, self.disc_plan(discs, d, construct_args, fields_open)))
            .collect();
        discs.plans = plans;
    }

    fn disc_plan(
        &self,
        discs: &Discs<'a>,
        d: &Disc<'a>,
        construct_args: &HashMap<&'a str, Vec<&'a [ExprId]>>,
        fields_open: bool,
    ) -> Option<Vec<ExprId>> {
        let class = d.class;
        // no subclass constructor writes the fields again
        let alone = fields_open
            && !self.foreign_classes.contains(class)
            && !matches!(self.ast.class_parents.get(class), Some(Some(_)))
            && !(self.ast.class_parents.keys()).any(|k| derives_from(self.ast, k, class));
        let ctor = self.ctors.iter().find(|k| k.class == class);
        let (true, Some(ctor), Some(fw)) = (alone, ctor, self.sole_write(discs, class, d.field))
        else {
            return None;
        };
        // the value a site stores through a prefix write
        let arg = |w: &PrefixWrite, args: &[ExprId]| -> Option<ExprId> {
            let Some(i) = w.param else {
                return Some(w.value);
            };
            let p = ctor.user.get(i)?;
            let plain = !p.is_rest && p.default.is_none() && !self.ctor_param_written(class, i);
            let spread = (args.iter().take(i + 1))
                .any(|a| matches!(self.ast.get_expr(*a), Expr::Spread { .. }));
            (plain && !spread).then(|| args.get(i).copied())?
        };
        let ruled: Vec<(&PrefixWrite, &Key)> = (d.ruled_out.iter())
            .filter_map(|(f, k)| Some((self.sole_write(discs, class, f)?, k)))
            .collect();
        let mut out = Vec::new();
        for args in construct_args.get(class).map_or(&[][..], Vec::as_slice) {
            let is_out = ruled.iter().any(|(w, k)| {
                let a = arg(w, args);
                a.and_then(|a| key(self.ast.get_expr(a), &discs.consts))
                    .as_ref()
                    == Some(*k)
            });
            if !is_out {
                out.push(arg(fw, args)?);
            }
        }
        Some(out)
    }

    /// A store reaches user parameter `i` of `class`'s constructor (a
    /// width twin's body is a copy, with the same stores).
    fn ctor_param_written(&self, class: &str, i: usize) -> bool {
        let Some(ctor) = self.ctors.iter().find(|k| k.class == class) else {
            return true;
        };
        let Some(p) = ctor.user.get(i).map(|p| p.name.as_str()) else {
            return true;
        };
        let vs = self.assign_values.get(p).map_or(&[][..], Vec::as_slice);
        vs.iter().any(|v| {
            self.scopes.reach(self.scopes.owner(*v), p).is_none_or(|r| {
                (r.params.iter()).any(|b| matches!(b, Binder::Ctor(k, j) if *k == class && *j == i))
            })
        })
    }

    /// The one constructor prefix write of `(class, field)`, when the
    /// only other writes are the same write in each width twin and the
    /// seeds those twins overwrite.
    fn sole_write(&self, discs: &Discs<'a>, class: &str, field: &str) -> Option<&PrefixWrite<'a>> {
        if self.deleted.contains(field) || self.foreign.contains(field) {
            return None;
        }
        let mut ws = (self.prefix.iter()).filter(|w| w.class == class && w.field == field);
        let (Some(w), None) = (ws.next(), ws.next()) else {
            return None;
        };
        let t = discs.twins.get(class)?;
        let tw: Vec<&PrefixWrite> = t.writes.iter().filter(|x| x.field == field).collect();
        let twins_agree =
            tw.len() == t.ctors && tw.iter().all(|x| x.param.is_some() && x.param == w.param);
        let writes = self.member_writes.get(field).map_or(&[][..], Vec::as_slice);
        let only_twins =
            (writes.iter()).all(|v| t.seeds.contains(v) || tw.iter().any(|x| x.value == *v));
        (twins_agree && only_twins).then_some(w)
    }
}

impl<'b> State<'_, 'b> {
    /// Every construction site of the read's class that its facts have
    /// not ruled out fills the field with a live value.
    pub(in super::super) fn disc_live(&self, e: ExprId, visiting: &mut Visiting<'b>) -> bool {
        let plan = self.discs.plans.get(&e).and_then(Option::as_ref);
        plan.is_some_and(|args| args.iter().all(|a| self.value_live(*a, visiting)))
    }

    pub(in super::super) fn proven_members(&self) -> ExprMask {
        let mut out = ExprMask::new(self.c.ast);
        for e in self.discs.plans.keys() {
            if self.disc_live(*e, &mut Visiting::default()) {
                out.insert(*e);
            }
        }
        out
    }
}
