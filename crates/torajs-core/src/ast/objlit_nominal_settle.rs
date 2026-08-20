//! Apply phase of [`super::objlit_nominal`] — what its collect loop
//! produced, landed on the AST. Split out at the 500-line file limit;
//! the receiver-shape decisions themselves stay documented at their
//! mint sites in the host.

use std::collections::HashMap;

use super::objlit_nominal::MethodPatch;
use super::{Expr, ExprId, Stmt};

/// What the collect loop above produced, handed to the apply phase in
/// one piece.
pub(super) struct Settle<'a> {
    pub(super) stmts: &'a mut Vec<Stmt>,
    pub(super) exprs: &'a mut Vec<Expr>,
    /// Anylane members promoted to the `__this: any` receiver-first
    /// shape — `(face ExprId, lifted fn name)`.
    pub(super) any_patches: Vec<(ExprId, String)>,
    /// Nominal members: the `__this: __ObjLit_n` receiver patches.
    pub(super) patches: Vec<MethodPatch>,
    /// The accessor subset of `patches` whose body never says `this`
    /// — marked receiver-first once the nominal patches land.
    pub(super) recvless_accessors: Vec<String>,
    /// The synthetic `__ObjLit_n` aliases, `__mth_placeholder` fields
    /// still parked in them.
    pub(super) type_decls: Vec<Stmt>,
    pub(super) fn_sigs: &'a mut HashMap<String, String>,
    pub(super) fnexpr_recv_fns: &'a mut std::collections::HashSet<String>,
    pub(super) sloppy: bool,
    pub(super) spans: &'a mut Vec<crate::lexer::Span>,
}

/// Apply phase — promote the anylane faces, then land the nominal
/// patches and publish the aliases they name.
pub(super) fn settle_collected(s: Settle<'_>) {
    if !s.any_patches.is_empty() {
        super::fnexpr_this::promote_recv_any(
            s.stmts,
            s.exprs,
            &s.any_patches,
            s.fnexpr_recv_fns,
            s.sloppy,
            s.spans,
        );
    }
    if s.patches.is_empty() {
        return;
    }
    let mut type_decls = s.type_decls;
    super::objlit_nominal::apply_patches(
        s.stmts,
        s.exprs,
        &s.patches,
        &mut type_decls,
        s.fn_sigs,
        s.fnexpr_recv_fns,
    );
    // Rotation 461 — an accessor takes the receiver whether or not it
    // says `this`, and a this-FREE one never READS the slot it was
    // given. Which lane the literal ends up on is not decidable here
    // (the checker's Any answer at a call argument is the leak every
    // any-lane leg keeps missing), so mark those faces receiver-first:
    // the dynobj accessor install then picks BOXED|RECV and its argv
    // lines up with the declared params, instead of hitting
    // `guard_anylane_recv_face`.
    //
    // The `__this` ANNOTATION deliberately stays nominal. It is not
    // dead metadata — the width analysis reads it to join a member fn
    // to its literal, and re-annotating it `any` split the caller's
    // slot projection from the body's own narrowing (measured: caller
    // passed the setter value in V0, callee read X3). The receiver
    // that arrives on the dynobj lane is a box the body never
    // dereferences, so the lie costs nothing there.
    s.fnexpr_recv_fns.extend(s.recvless_accessors);
    s.stmts.extend(type_decls);
}
