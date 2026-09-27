//! The jump targets of the generator state-machine emitter: one
//! [`SmJump`] per yield-bearing construct a `break` / `continue` can
//! name, and the rule that resolves a jump against them. Moved out of
//! `desugar_generators_sm` for file size.

/// One entry of [`GenSm::loop_stack`]: where a `continue` / `break`
/// aimed at this construct gotos. A yield-loop has both; a labeled
/// block holding a yield (ES §13.13 — `lbl: { …; break lbl; … }`,
/// which the states erase) has only its exit, and a BARE jump never
/// targets it (`cont: None` also marks it for the bare-jump skip).
/// `label` is `Some` when the construct sat under a `Stmt::Labeled`.
#[derive(Clone)]
pub(super) struct SmJump {
    pub(super) cont: Option<usize>,
    pub(super) brk: usize,
    pub(super) label: Option<String>,
}

/// The resolution behind [`GenSm::sm_loop_target`] and the inline
/// jump rewriter: bare → innermost yield-LOOP (blocks skipped);
/// labeled → the entry carrying that label, whose continue target a
/// block does not have (`continue` naming a block is an early error,
/// so that read never happens).
pub(super) fn sm_jump_target(
    stack: &[SmJump],
    label: &Option<String>,
    want_break: bool,
) -> Option<usize> {
    let pick = |j: &SmJump| if want_break { Some(j.brk) } else { j.cont };
    match label {
        None => stack.iter().rev().find(|j| j.cont.is_some()).and_then(pick),
        Some(l) => stack
            .iter()
            .rev()
            .find(|j| j.label.as_deref() == Some(l.as_str()))
            .and_then(pick),
    }
}
