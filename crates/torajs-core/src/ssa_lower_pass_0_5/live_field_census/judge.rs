//! The judgement half of the census: the fixed point over candidate
//! fields and functions, and what "live by construction" means for a
//! value, a constructor parameter and a function's returns.

use crate::ssa::id_hash::{HashMap, HashSet};

use crate::ast::desugar_async_sm_walk::visit_stmt_exprs;
use crate::ast::{ExprId, Stmt};

use super::disc::Discs;
use super::scope::Owner;
use super::walk::{Few, collect_super_args, derives_from, own_returns, pointer_shaped};
use super::{Census, Live, LiveFields};

/// A read or a constructor parameter met again on the way, judged by
/// its other sources.
#[derive(PartialEq, Eq, Hash)]
pub(super) enum Visit<'b> {
    Read(Owner, &'b str),
    Ctor(usize, usize),
}

pub(super) type Visiting<'b> = HashSet<Visit<'b>>;

impl<'a> Census<'a> {
    /// Greatest fixed point over the candidate fields, functions and
    /// parameters; `fields_open` says no road the census cannot see
    /// reaches a field.
    pub(super) fn solve(&self, fields_open: bool) -> Live {
        let construct_args = &self.construct_args;
        let mut live = self.candidate_fields(fields_open);
        let mut live_fns: HashSet<&str> = self.fns.keys().copied().collect();
        let judged = self.judged_params();
        let entry = self.entry_checked(&judged);
        let checked: HashSet<(&str, usize)> = (entry.iter())
            .flat_map(|(f, at)| at.iter().map(|i| (*f, *i)))
            .collect();
        let mut live_params = judged;
        // what the rounds read again and again, gathered once; a fact
        // needs a test, and a plan an instance of a class no road reaches
        let mut discs = Discs::default();
        if fields_open && self.disc_test && !self.ctors.is_empty() {
            discs = self.discs();
            self.disc_plans(&mut discs, construct_args, fields_open);
        }
        let (methods, rets) = (self.methods(), self.returns());
        let (unproven, members, entry) = loop {
            let st = State {
                c: self,
                scratch: Default::default(),
                live: &live,
                live_fns: &live_fns,
                live_params: &live_params,
                construct_args,
                discs: &discs,
                methods: &methods,
                rets: &rets,
                fields_open,
            };
            let next_live: HashSet<(&str, &str)> = (live.iter())
                .filter(|(c, f)| st.field_live(c, f))
                .copied()
                .collect();
            let next_fns: HashSet<&str> =
                live_fns.iter().filter(|f| st.fn_live(f)).copied().collect();
            let next_params: HashSet<(&str, usize)> = (live_params.iter())
                .filter(|p| checked.contains(*p) || st.param_live(p.0, p.1))
                .copied()
                .collect();
            if next_live.len() == live.len()
                && next_fns.len() == live_fns.len()
                && next_params.len() == live_params.len()
            {
                break st.reads(&entry);
            }
            live = next_live;
            live_fns = next_fns;
            live_params = next_params;
        };
        Live {
            fields: self.with_derived(&live),
            unproven,
            members,
            entry,
        }
    }

    /// The fields the fixed point starts from: every prefix write of a
    /// field declared non-nullable and pointer-shaped.
    fn candidate_fields(&self, fields_open: bool) -> HashSet<(&'a str, &'a str)> {
        if !fields_open {
            return HashSet::default();
        }
        (self.prefix.iter())
            .filter(|w| !self.foreign_classes.contains(w.class))
            .filter(|w| self.field_type_live(w.class, w.field))
            .map(|w| (w.class, w.field))
            .collect()
    }

    /// The `super(..)` argument lists, which construct the parent, join
    /// the `new C(..)` ones.
    pub(super) fn add_super_sites(&mut self) {
        let (ast, sites) = (self.ast, &mut self.construct_args);
        for ctor in &self.ctors {
            let Some(Some(parent)) = ast.class_parents.get(ctor.class) else {
                continue;
            };
            for s in ctor.body {
                visit_stmt_exprs(s, &mut |e| {
                    collect_super_args(ast, e, &mut |args| {
                        sites.entry(parent.as_str()).or_default().push(args);
                    });
                });
            }
        }
    }

    /// The field is declared on the class with a non-nullable
    /// pointer-shaped type.
    fn field_type_live(&self, class: &str, field: &str) -> bool {
        self.ast.stmts.iter().any(|s| {
            let Stmt::TypeDecl { name, fields, .. } = s else {
                return false;
            };
            name == class
                && fields.iter().any(|(k, ann)| {
                    k.lossy() == field
                        && !ann.starts_with("__nullable(")
                        && !matches!(ann.as_str(), "any" | "undefined" | "void" | "null")
                })
        }) && self.prefix.iter().any(|w| {
            w.class == class && w.field == field && self.ty(w.value).is_some_and(pointer_shaped)
        })
    }

    /// The `(function, index)` parameters judged by call sites.
    pub(super) fn judged_params(&self) -> HashSet<(&'a str, usize)> {
        if self.calls.open {
            return HashSet::default();
        }
        (self.fn_params.values().flat_map(Few::as_slice))
            .filter(|(f, _)| !self.calls.value_used.contains(f) && !self.assigned.contains(f))
            .copied()
            .collect()
    }

    /// Each declared function's returned values, or none when one of
    /// its returns has no value.
    pub(super) fn returns(&self) -> HashMap<&'a str, Option<Vec<ExprId>>> {
        let mut out = HashMap::default();
        for (f, (_, body)) in &self.fns {
            let mut v = Some(Vec::new());
            own_returns(body, &mut |r| match (r, v.as_mut()) {
                (Some(e), Some(v)) => v.push(e),
                _ => v = None,
            });
            out.insert(*f, v);
        }
        out
    }

    /// The live fields, with each also under the name of every class
    /// deriving from its own: a derived class reads the field through
    /// its own class name.
    pub(super) fn with_derived(&self, live: &HashSet<(&str, &str)>) -> LiveFields {
        let mut fields: LiveFields = (live.iter())
            .map(|(c, f)| (c.to_string(), f.to_string()))
            .collect();
        for (class, field) in live {
            for d in self.ast.class_parents.keys() {
                if derives_from(self.ast, d, class) && !self.foreign_classes.contains(d.as_str()) {
                    fields.insert((d.clone(), field.to_string()));
                }
            }
        }
        fields
    }
}

pub(super) struct State<'a, 'b> {
    pub(super) c: &'b Census<'a>,
    /// The visiting set the judgements that start a walk lend each
    /// other: a walk leaves it as it found it.
    pub(super) scratch: std::cell::RefCell<Visiting<'b>>,
    pub(super) live: &'b HashSet<(&'a str, &'a str)>,
    pub(super) live_fns: &'b HashSet<&'a str>,
    pub(super) live_params: &'b HashSet<(&'a str, usize)>,
    pub(super) construct_args: &'b HashMap<&'a str, Few<&'a [ExprId]>>,
    pub(super) discs: &'b Discs<'a>,
    pub(super) methods: &'b super::fresh::Methods<'b>,
    pub(super) rets: &'b HashMap<&'a str, Option<Vec<ExprId>>>,
    /// No road the census cannot see reaches a field or a method.
    pub(super) fields_open: bool,
}
