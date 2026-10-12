//! Which rewritten declarations a statement above them can reach.
//!
//! §10.2.11 FunctionDeclarationInstantiation initializes every function
//! declaration of a body before its first statement runs (§14.2.3 does
//! the same for a block), so a call that precedes the declaration's text
//! is legal. Rewritten into a `let`, the declaration is live only from
//! its slot on, so a binding some earlier statement can reach has to
//! move above that statement — directly named there, or reached
//! through a peer that statement calls (`r = g();` with `g` calling a
//! later `f` needs both live).
//!
//! Only those move. A binding nothing above names stays in its slot:
//! moving it would turn every binding it captures that is declared
//! between the new and the old slot from a backward capture into a
//! forward one, and a forward capture is boxed.

use crate::ast::free_vars::free_vars_of_body;
use crate::ast::{Ast, Stmt};

/// The statement the early sites move above, and the sites themselves
/// (indices into the list, ascending).
pub(in crate::ast) struct EarlyUse {
    first_use: usize,
    pub(in crate::ast) sites: Vec<usize>,
}

/// `routed` are the ascending indices of the list's rewritten `let`s.
/// For each other statement, the names it references are closed over
/// the peer edges (a rewritten body naming another rewritten binding);
/// every site so reached that sits below the statement is early.
pub(in crate::ast) fn early_use(ast: &Ast, stmts: &[Stmt], routed: &[usize]) -> Option<EarlyUse> {
    let &last = routed.last()?;
    let names: Vec<&str> = routed
        .iter()
        .map(|i| match &stmts[*i] {
            Stmt::LetDecl { name, .. } => name.as_str(),
            _ => unreachable!("a routed site is a rewritten let"),
        })
        .collect();
    let peers_named = |s: &Stmt| -> Vec<usize> {
        let free = free_vars_of_body(ast, &[], std::slice::from_ref(s));
        (0..names.len())
            .filter(|k| free.iter().any(|n| n == names[*k]))
            .collect()
    };
    let edges: Vec<Vec<usize>> = routed.iter().map(|i| peers_named(&stmts[*i])).collect();
    let mut first_use: Option<usize> = None;
    let mut early: Vec<bool> = vec![false; routed.len()];
    for (p, s) in stmts.iter().enumerate().take(last) {
        if routed.contains(&p) {
            continue;
        }
        let mut reached: Vec<usize> = Vec::new();
        let mut work = peers_named(s);
        while let Some(k) = work.pop() {
            if reached.contains(&k) {
                continue;
            }
            reached.push(k);
            work.extend(edges[k].iter().copied());
        }
        for k in reached {
            if routed[k] > p {
                early[k] = true;
                first_use.get_or_insert(p);
            }
        }
    }
    let first_use = first_use?;
    let sites = (0..routed.len())
        .filter(|k| early[*k])
        .map(|k| routed[k])
        .collect();
    Some(EarlyUse { first_use, sites })
}

impl EarlyUse {
    /// Keep only the sites `keep` accepts; `None` when none is left. The
    /// destination stays above the first use, which is still above them.
    pub(in crate::ast) fn retain(mut self, keep: impl Fn(usize) -> bool) -> Option<EarlyUse> {
        self.sites.retain(|i| keep(*i));
        (!self.sites.is_empty()).then_some(self)
    }
}

/// Move the early sites, in order, to just above the first statement
/// that reaches one of them.
pub(in crate::ast) fn raise_early(stmts: &mut [Stmt], early: &EarlyUse) {
    let mut dest = early.first_use;
    for &i in &early.sites {
        stmts[dest..=i].rotate_right(1);
        dest += 1;
    }
}

/// [`early_use`] then [`raise_early`], for a list whose rewritten `let`s
/// all stay in the list.
pub(in crate::ast) fn hoist_above_first_use(ast: &Ast, stmts: &mut [Stmt], routed: &[usize]) {
    if let Some(early) = early_use(ast, stmts, routed) {
        raise_early(stmts, &early);
    }
}
