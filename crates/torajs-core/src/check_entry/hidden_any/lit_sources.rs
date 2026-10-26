//! The function declarations an object literal's member values are, as
//! far as the source shows them (`super::method_lits`).

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

use super::value_places::{Place, places};
use crate::ast::{Ast, Expr, ExprId, Stmt};

/// The function declarations an object literal's member value is: a
/// closure, a function declared once whose return a round may retype,
/// a `const` binding declared once, never assigned and written without
/// an annotation whose initializer is one, or a conditional both of
/// whose arms are. A function is followed only to the member values
/// it reaches: one also called directly, stored into an annotated
/// binding, handed to a call or read anywhere else keeps its return.
pub(super) struct Sources {
    consts: HashMap<String, ExprId>,
    fns: HashSet<String>,
    /// the member values each function reaches, `None` when it reaches
    /// another place too; read off the program the first time a
    /// member value is asked about, which most programs never do
    sites: OnceCell<HashMap<String, Option<HashSet<ExprId>>>>,
    /// every expression's place, read off the program as `sites` is
    places: OnceCell<HashMap<ExprId, Place>>,
    /// the initializers of the bindings written with an annotation
    annotated: OnceCell<HashSet<ExprId>>,
}

impl Sources {
    pub(super) fn of(ast: &Ast) -> Self {
        let count = declarations(ast);
        let once = |n: &String| count.get(n) == Some(&1);
        let mut consts = HashMap::new();
        let mut fns = HashSet::new();
        crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
            Stmt::LetDecl {
                name,
                mutable: false,
                type_ann: None,
                init,
                ..
            } if once(name) => {
                consts.insert(name.clone(), *init);
            }
            Stmt::FnDecl {
                name,
                type_params,
                return_type,
                is_generator,
                ..
            } if once(name) => {
                let scalar = |a: &str| matches!(a, "number" | "string" | "boolean");
                if !is_generator
                    && type_params.is_empty()
                    && !ast.async_fns.contains(name)
                    && return_type.as_deref().is_none_or(scalar)
                {
                    fns.insert(name.clone());
                }
            }
            _ => {}
        });
        Sources {
            consts,
            fns,
            sites: OnceCell::new(),
            places: OnceCell::new(),
            annotated: OnceCell::new(),
        }
    }

    pub(super) fn places(&self, ast: &Ast) -> &HashMap<ExprId, Place> {
        self.places.get_or_init(|| places(ast))
    }

    /// Whether `e` initializes a binding written with an annotation.
    pub(super) fn annotated_init(&self, ast: &Ast, e: ExprId) -> bool {
        let inits = self.annotated.get_or_init(|| {
            let mut out = HashSet::new();
            crate::ast::walk_stmts(&ast.stmts, &mut |s| {
                if let Stmt::LetDecl {
                    type_ann: Some(_),
                    init,
                    ..
                } = s
                {
                    out.insert(*init);
                }
            });
            out
        });
        inits.contains(&e)
    }

    /// The functions the value `v` is, or `None` when it may be another
    /// function value or one of them reaches a member value other than
    /// those of `allowed`.
    pub(super) fn of_value(
        &self,
        ast: &Ast,
        v: ExprId,
        allowed: &HashSet<ExprId>,
    ) -> Option<Vec<String>> {
        let mut out = Vec::new();
        let mut todo = vec![v];
        // a chain of `const` bindings is no longer than the bindings
        let mut budget = self.consts.len() + 1;
        while let Some(v) = todo.pop() {
            let f = match ast.get_expr(v) {
                Expr::Closure { fn_name, .. } => fn_name,
                Expr::Ident(n) if self.fns.contains(n) => n,
                Expr::Ident(n) if budget > 0 => {
                    budget -= 1;
                    todo.push(*self.consts.get(n)?);
                    continue;
                }
                Expr::Ternary {
                    then_branch,
                    else_branch,
                    ..
                } => {
                    todo.extend([*then_branch, *else_branch]);
                    continue;
                }
                _ => return None,
            };
            let all = (self.sites)
                .get_or_init(|| Reach::new(ast, self.places(ast), &self.consts, &self.fns).all());
            let sites = all.get(f)?.as_ref()?;
            // one written in a member reaches at least that member
            if sites.is_empty() || !sites.is_subset(allowed) {
                return None;
            }
            out.push(f.clone());
        }
        Some(out)
    }
}

/// How many times each name is declared or assigned.
fn declarations(ast: &Ast) -> HashMap<String, usize> {
    let mut count: HashMap<String, usize> = HashMap::new();
    let mut add = |n: &String| *count.entry(n.clone()).or_default() += 1;
    crate::ast::walk_stmts(&ast.stmts, &mut |s| match s {
        Stmt::LetDecl { name, .. } => add(name),
        Stmt::FnDecl { name, params, .. } => {
            add(name);
            params.iter().for_each(|p| add(&p.name));
        }
        Stmt::ForOf { var_name, .. }
        | Stmt::ForOfSplitIter { var_name, .. }
        | Stmt::UsingDecl { name: var_name, .. }
        | Stmt::Try {
            catch_param: Some(var_name),
            ..
        } => add(var_name),
        _ => {}
    });
    for e in &ast.exprs {
        if let Expr::Assign { target, .. } = e
            && let Expr::Ident(n) = ast.get_expr(*target)
        {
            add(n);
        }
    }
    count
}

/// The member values the functions and `const` bindings of a program
/// reach, read off the places their values go.
struct Reach<'a> {
    ast: &'a Ast,
    places: &'a HashMap<ExprId, Place>,
    /// a `const` binding's initializer → the binding
    inits: HashMap<ExprId, &'a String>,
    /// the reads of each function or `const` binding by name
    reads: HashMap<&'a String, Vec<ExprId>>,
    /// the closures each lifted function is written as
    closures: HashMap<&'a String, Vec<ExprId>>,
    fns: &'a HashSet<String>,
    /// the bindings being followed, against a chain that loops
    open: HashSet<&'a String>,
}

impl<'a> Reach<'a> {
    fn new(
        ast: &'a Ast,
        places: &'a HashMap<ExprId, Place>,
        consts: &'a HashMap<String, ExprId>,
        fns: &'a HashSet<String>,
    ) -> Self {
        let mut reads: HashMap<&String, Vec<ExprId>> = HashMap::new();
        let mut closures: HashMap<&String, Vec<ExprId>> = HashMap::new();
        for (i, e) in ast.exprs.iter().enumerate() {
            match e {
                Expr::Ident(n) => {
                    if let Some((k, _)) = consts.get_key_value(n) {
                        reads.entry(k).or_default().push(ExprId(i as u32));
                    } else if let Some(k) = fns.get(n) {
                        reads.entry(k).or_default().push(ExprId(i as u32));
                    }
                }
                Expr::Closure { fn_name, .. } => {
                    closures.entry(fn_name).or_default().push(ExprId(i as u32))
                }
                _ => {}
            }
        }
        Reach {
            ast,
            places,
            inits: consts.iter().map(|(n, e)| (*e, n)).collect(),
            reads,
            closures,
            fns,
            open: HashSet::new(),
        }
    }

    /// The member values every function reaches, through the closures
    /// it is written as and the reads of its name both: a function the
    /// compiler lifts (a closure, the `__forward_<f>` relaying a
    /// declaration read as a value) is declared and written as a
    /// closure too.
    fn all(mut self) -> HashMap<String, Option<HashSet<ExprId>>> {
        let mut es = std::mem::take(&mut self.closures);
        for f in self.fns {
            es.entry(f).or_default();
        }
        (es.into_iter())
            .map(|(f, mut es)| {
                es.extend(self.reads.get(f).into_iter().flatten());
                (f.clone(), self.union(&es))
            })
            .collect()
    }

    fn union(&mut self, es: &[ExprId]) -> Option<HashSet<ExprId>> {
        let mut out = HashSet::new();
        for e in es {
            out.extend(self.site(*e)?);
        }
        Some(out)
    }

    /// The member values the value `e` reaches: the one it is, through
    /// the arms of conditionals, or those the `const` binding it
    /// initializes reaches.
    fn site(&mut self, mut e: ExprId) -> Option<HashSet<ExprId>> {
        loop {
            match self.places.get(&e)? {
                Place::Expr(p) => match self.ast.get_expr(*p) {
                    Expr::ObjectLit { .. } => return Some(HashSet::from([e])),
                    Expr::Ternary { cond, .. } if *cond != e => e = *p,
                    _ => return None,
                },
                Place::Bind(None) => {
                    let c = *self.inits.get(&e)?;
                    if !self.open.insert(c) {
                        return None;
                    }
                    let es = self.reads.get(c).cloned().unwrap_or_default();
                    let out = self.union(&es);
                    self.open.remove(c);
                    return out;
                }
                _ => return None,
            }
        }
    }
}
