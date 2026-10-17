//! Scope / borrow-move tracking method cluster (chunk 425).
//!
//! The Checker methods that manage scopes and the move ledger:
//! - declare / lookup / lookup_with_depth — scope-stack bindings
//! - is_descendant_of — class hierarchy walk
//! - mark_moved / mark_unmoved / snapshot_moved / restore_moved /
//!   join_branch_moves — move-ledger state machine across branches
//! - consume / consume_escape — use-site ownership consumption
//! - classify_init_alias — alias-vs-share init classification

use super::*;

impl Checker {
    pub(crate) fn declare(&mut self, name: String, info: LocalInfo) -> Result<(), String> {
        let top = self
            .scopes
            .last_mut()
            .expect("at least one scope is always present");
        if top.contains_key(&name) {
            return Err(format!("redeclaration of `{name}` in current scope"));
        }
        top.insert(name, info);
        Ok(())
    }

    pub(crate) fn lookup(&self, name: &str) -> Option<LocalInfo> {
        for s in self.scopes.iter().rev() {
            if let Some(i) = s.get(name) {
                return Some(i.clone());
            }
        }
        None
    }

    /// Like `lookup` but also returns the scope depth at which the binding
    /// was found (0 = outermost / fn-root, `scopes.len() - 1` = innermost).
    /// M1.3 uses this to detect cross-scope `let n = s` cases — an Ident
    /// init from an outer scope is treated as alias-only (n borrows s's
    /// heap, both stay readable; no ownership transfer that would dangle
    /// the outer reference at this block's close).
    fn lookup_with_depth(&self, name: &str) -> Option<(LocalInfo, usize)> {
        for (i, s) in self.scopes.iter().enumerate().rev() {
            if let Some(info) = s.get(name) {
                return Some((info.clone(), i));
            }
        }
        None
    }

    /// M-OO.5 — true iff `child` is a descendant of `ancestor` along
    /// the class inheritance chain stored in `ast.class_parents`.
    /// Used by Protected visibility enforcement: `protected member`
    /// access is allowed when the caller's class is the owner OR any
    /// subclass.
    pub(crate) fn is_descendant_of(&self, ast: &Ast, child: &str, ancestor: &str) -> bool {
        let mut cur = child;
        // Hop bound doubles as a cycle guard — a mutual-extends cycle
        // in class_parents must not spin the walk (the declared-before
        // rule rejects such programs, but this helper can run first).
        let mut hops = ast.class_parents.len() + 1;
        while let Some(parent) = ast.class_parents.get(cur).and_then(|p| p.as_deref()) {
            if parent == ancestor {
                return true;
            }
            cur = parent;
            hops -= 1;
            if hops == 0 {
                break;
            }
        }
        false
    }

    /// Walk the scope stack from innermost outward and flip `moved=true`
    /// on the first matching binding. Caller must already have verified
    /// the binding exists.
    fn mark_moved(&mut self, name: &str) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.moved = true;
                return;
            }
        }
    }

    /// Inverse of `mark_moved` — the binding's slot now owns a fresh value
    /// (Assign rebound it). Used to clear any transient `moved` state set
    /// during rhs evaluation. Lets `s = s + "x"` work: the BinOp internally
    /// consumes s (because str+str consumes both), then Assign rebinds s
    /// with the concat result, so subsequent reads of s are fine.
    pub(crate) fn mark_unmoved(&mut self, name: &str) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(info) = s.get_mut(name) {
                info.moved = false;
                return;
            }
        }
    }

    /// Snapshot every (scope_idx, name) → moved bool across the whole
    /// scope stack. Used by CFG-aware branch checking: snapshot before
    /// a branch, run the branch (which may mark bindings moved), then
    /// either restore the snapshot (diverging branch) or merge the
    /// captured post-state with sibling branches' post-states.
    pub(crate) fn snapshot_moved(&self) -> Vec<Vec<(String, bool)>> {
        self.scopes
            .iter()
            .map(|s| s.iter().map(|(n, i)| (n.clone(), i.moved)).collect())
            .collect()
    }

    /// Restore moved flags to the values captured by `snapshot_moved`.
    /// Bindings introduced after the snapshot (i.e. inside the branch)
    /// are unaffected — they're either still in the scope or already
    /// popped by branch teardown.
    pub(crate) fn restore_moved(&mut self, snap: &[Vec<(String, bool)>]) {
        for (scope, snap_scope) in self.scopes.iter_mut().zip(snap.iter()) {
            for (n, m) in snap_scope {
                if let Some(info) = scope.get_mut(n) {
                    info.moved = *m;
                }
            }
        }
    }

    /// Apply the join of two branches' post-move states to the current
    /// scope stack. A binding is marked moved post-join iff every
    /// non-diverging branch moved it. Diverging branches contribute no
    /// post-join moves (their moves go off with the diverging exit).
    /// `pre` is the snapshot taken before either branch ran; `then_post`
    /// / `else_post` are the snapshots taken after each branch ran (or
    /// None for an absent else, which is treated as "live, no moves").
    pub(crate) fn join_branch_moves(
        &mut self,
        pre: &[Vec<(String, bool)>],
        then_post: &[Vec<(String, bool)>],
        then_div: bool,
        else_post: Option<&[Vec<(String, bool)>]>,
        else_div: bool,
    ) {
        // For each scope frame and binding, compute newly-moved-in-branch
        // (post.moved && !pre.moved) for each side, then join.
        for (scope_idx, pre_scope) in pre.iter().enumerate() {
            for (name, pre_moved) in pre_scope {
                if *pre_moved {
                    // Already moved before the if; nothing changes.
                    continue;
                }
                let then_moved = then_post.get(scope_idx).is_some_and(|s| {
                    s.iter()
                        .find(|(n, _)| n == name)
                        .map(|(_, m)| *m)
                        .unwrap_or(false)
                });
                let else_moved = match else_post {
                    Some(es) => es.get(scope_idx).is_some_and(|s| {
                        s.iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, m)| *m)
                            .unwrap_or(false)
                    }),
                    // Absent else = implicit empty path that didn't move.
                    None => false,
                };
                let then_lives = !then_div;
                let else_lives = match else_post {
                    Some(_) => !else_div,
                    None => true,
                };
                let join_moved = match (then_lives, else_lives) {
                    // Both diverge → post-if unreachable. Pre-state survives.
                    (false, false) => *pre_moved,
                    // Only else lives → propagate else's moves.
                    (false, true) => else_moved,
                    // Only then lives → propagate then's moves.
                    (true, false) => then_moved,
                    // Both live → conservative intersection (require both
                    // sides to consume for post-state to be moved).
                    (true, true) => then_moved && else_moved,
                };
                if join_moved && !pre_moved {
                    // Mark in the right scope (we know it's at scope_idx).
                    if let Some(scope) = self.scopes.get_mut(scope_idx)
                        && let Some(info) = scope.get_mut(name)
                    {
                        info.moved = true;
                    }
                }
            }
        }
    }

    /// Try to transfer ownership FROM the given expression. Called at the
    /// four transfer sites: let-rhs, assign-rhs, non-Copy fn arg, return
    /// value, struct field write.
    ///
    /// TS-shape semantics: `let n = s; console.log(s);` works — both
    /// bindings read the same heap.
    ///
    /// **A second transfer is no longer an error.** The reject that used
    /// to stand here said, in its own commit message, that it "catches
    /// multi-rooted ownership at compile time without needing runtime
    /// support" — it was standing in for a retain contract the lowering
    /// did not have. RFC `20260705-operand-ownership` retired that reject
    /// on four lanes as the contract landed (assign-ident, assign-member,
    /// index-assign, consuming params, −440 lines); throw and return were
    /// the two stops it never reached. And throw never needed it:
    /// `ssa_lower_stmt_throw` retains into the throw slot and leaves
    /// every binding its drop, so the ledger's only observable effect
    /// was the message. Ownership is not a concept ECMA-262 has, nor one
    /// TypeScript has — see `docs/language-spec/ch1-type-runtime-boundary.md`
    /// §3, family F9.
    ///
    /// Member / Index reads of obj's field are NOT transfers — the field's
    /// heap is owned by obj, and the new binding is an alias (handled at
    /// the LetDecl site via `classify_init_alias`, not here).
    pub(crate) fn consume(&mut self, ast: &Ast, eid: ExprId) {
        if let Expr::Ident(name) = ast.get_expr(eid) {
            let name = name.clone();
            if let Some(info) = self.lookup(&name) {
                if info.ty.is_copy() {
                    return;
                }
                self.mark_moved(&name);
            }
        }
    }

    /// Transfer at a scope-exit boundary (return / throw). Unlike the
    /// mid-scope `consume`, a borrowed alias is legal here: the binding
    /// dies with the scope and ssa_lower retains at the boundary
    /// (retain-at-return / retain-at-throw), so the escaping reference
    /// carries its own +1 while the canonical owner keeps its stake.
    /// Owned bindings still consume — their stake transfers out.
    pub(crate) fn consume_escape(&mut self, ast: &Ast, eid: ExprId) {
        if let Expr::Ident(name) = ast.get_expr(eid)
            && let Some(info) = self.lookup(name)
            && info.borrowed
            && !info.ty.is_copy()
        {
            return;
        }
        self.consume(ast, eid);
    }

    /// Decide whether a let-bound or struct-field's init expression
    /// produces a fresh-owned value or aliases an existing one. Member
    /// and Index reads (`obj.field`, `arr[i]`) yield aliases — the heap
    /// is still owned by obj/arr; the new binding just holds a pointer
    /// for shared-read access. M1.3 extends this to cross-scope Ident
    /// init: when `s` lives in an outer scope, `let n = s` becomes an
    /// alias (otherwise transferring would dangle the outer reference
    /// when the inner block's drop fires). Same-scope Ident init is a
    /// SHARE — ssa_lower retains at the binding site so both bindings
    /// own independent stakes; neither is an alias nor consumed.
    /// Fresh-value init (literal, Call return, BinOp, ObjectLit, Array)
    /// produces a new owner; not an alias.
    pub(crate) fn classify_init_alias(&self, ast: &Ast, eid: ExprId) -> bool {
        match ast.get_expr(eid) {
            // String indexing returns a fresh owned Substr view
            // (chunk 561) — a new owner, not an element alias; the
            // receiver's memoized type is present because the init
            // was type-checked before classification. Missing entry
            // keeps the conservative alias answer.
            Expr::Index { obj, .. } => !matches!(self.expr_types.get(obj), Some(Type::String)),
            Expr::Member { .. } => true,
            Expr::Ident(name) => {
                if let Some((_, src_depth)) = self.lookup_with_depth(name) {
                    let cur_depth = self.scopes.len() - 1;
                    src_depth < cur_depth
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}
