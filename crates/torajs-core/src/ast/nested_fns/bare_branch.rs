//! The statement half of the nested-fn lift: a declaration standing
//! directly in a statement position (`if (c) function f() {}`, the
//! §B.3.2 / §B.3.4 shape) is lifted here, and anything else is handed
//! to the descent. Moved out of the parent when it neared the file-size
//! limit; the parent keeps the per-list lift.

use super::super::Stmt;
use super::super::annexb_fn_var::{AnnexB, LiftCtx, LiftMode};
use super::super::nested_fns_walk::descend_into_children;
use std::collections::{HashMap, HashSet};

/// P3.4-followup-A — recursive helper that descends into a single stmt
/// looking for nested FnDecls (block/if/while/for/try/switch
/// children). When found, lift to `lifted` with mangled name and
/// drop from the original site (replaced with a no-op Block).
pub(in crate::ast) fn collect_nested_fns_in_stmt(
    stmt: &mut Stmt,
    parent_name: &str,
    renames: &mut HashMap<String, String>,
    branch_scoped: &mut HashSet<String>,
    mode: LiftMode,
    lifted: &mut Vec<Stmt>,
    ctx: &mut LiftCtx,
) {
    // Bare-FnDecl-as-statement: `if (cond) function f() {}` parses
    // the then-branch as Stmt::FnDecl directly (no enclosing Block).
    // Same lift-and-mangle as the Block case but in-place.
    if let Stmt::FnDecl { name, span, .. } = stmt
        && !name.starts_with("__closure_")
        && !name.starts_with("__cm_")
        && !name.starts_with("__sm_")
        && !name.starts_with("__nested_")
    {
        let mangled = format!("__nested_{parent_name}_{name}_{}", ctx.counter);
        ctx.counter += 1;
        ctx.source_names.push((mangled.clone(), name.clone()));
        renames.insert(name.clone(), mangled.clone());
        // §B.3.3 in SLOPPY code: the outer name is a `var` binding
        // of its own, written where the declaration sits — so the
        // outer references must NOT be redirected to the mangled
        // block binding, and the slot the declaration vacates holds
        // that write. Strict code has no var binding: TS semantics
        // INSIDE a fn body (strict_branch) keep the block-scoped
        // reject (bun answers ReferenceError there), and strict
        // MODULE-TOP keeps the historical leak-to-parent rewrite.
        // The strict split is an S5.7 boundary-decision entry.
        // A declaration reached in this position is the branch of
        // an `if` / a loop / a label — a block by construction,
        // whatever the caller's own depth was.
        let verdict = ctx.annexb(name, *span, mode.descend());
        // A BARE-branch declaration (`if (c) function f() {}`) is
        // §B.3.2's shape, not §14.2.10's, and it keeps the answer
        // it had: there is no block here for a binding of its own
        // to live in. `AnnexB::Strict` therefore reads as `Legacy`
        // on this path.
        //
        // It also cannot be decided from the program's goal alone.
        // An INDIRECT eval's code is global SLOPPY code whatever
        // the calling program was (§19.2.1.1 step 3), so
        // `(0, eval)("if (true) function ib(){}")` inside a module
        // wants Annex B — and the eval's own goal is not something
        // this pass can see: `desugar_eval` blanks every spliced
        // declaration's span to the same `(0, 0)` sentinel every
        // synthesized declaration carries, so there is nothing to
        // ask. Same missing marker as the §B.3.3.3 parameterNames
        // item; both wait on the same design.
        if !matches!(verdict, AnnexB::Legacy | AnnexB::Strict) || mode.in_fn_body {
            branch_scoped.insert(name.clone());
        }
        let slot = match verdict {
            AnnexB::Apply => ctx.annexb_var_decl(name, &mangled),
            AnnexB::Skip | AnnexB::Strict | AnnexB::Legacy => Stmt::Block(Vec::new()),
        };
        // Take the FnDecl out of the slot, push the renamed decl
        // into `lifted`.
        let taken = std::mem::replace(stmt, slot);
        if let Stmt::FnDecl {
            type_params,
            params,
            return_type,
            body,
            is_generator,
            span,
            ..
        } = taken
        {
            lifted.push(Stmt::FnDecl {
                name: mangled,
                type_params,
                params,
                return_type,
                body,
                is_generator,
                // lift keeps the user's declaration text (B1b)
                span,
            });
        }
        return;
    }
    descend_into_children(stmt, parent_name, renames, branch_scoped, mode, lifted, ctx);
}
