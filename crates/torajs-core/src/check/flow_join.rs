//! Joining the flow narrows where control arrives from more than one
//! place: the exit of a loop, a `switch` or a labeled block a `break`
//! leaves, the entry of a `catch` or `finally`, and the statement after
//! a `try`. The walk is single-pass and in source order, so at such a
//! point its state describes only the last way in.

use super::narrows::GuardTypes;
use super::*;

/// The flow state at a point another one joins: the live type of every
/// binding that is possibly null there or carries a `flow_declared`
/// type, and the member narrows.
#[derive(Clone)]
pub(crate) struct FlowState {
    bindings: GuardTypes,
    member: HashMap<(String, String), Type>,
}

impl Checker {
    /// The flow state here, keyed for joining with a later point.
    pub(crate) fn flow_entry(&self) -> FlowState {
        let mut bindings = Vec::new();
        for (d, scope) in self.scopes.iter().enumerate() {
            for (n, info) in scope {
                if matches!(info.ty, Type::Nullable(_)) || info.flow_declared.is_some() {
                    bindings.push(((d, n.clone()), info.ty.clone()));
                }
            }
        }
        FlowState {
            bindings,
            member: self.narrows.member.clone(),
        }
    }

    /// The live types of the bindings `entry` tracks.
    pub(crate) fn flow_at(&self, entry: &FlowState) -> FlowState {
        let bindings = entry
            .bindings
            .iter()
            .filter_map(|(k, _)| Some((k.clone(), self.scopes.get(k.0)?.get(&k.1)?.ty.clone())))
            .collect();
        FlowState {
            bindings,
            member: self.narrows.member.clone(),
        }
    }

    pub(crate) fn restore_flow(&mut self, state: &FlowState) {
        self.guard_restore(&state.bindings);
        self.narrows.member = state.member.clone();
    }

    /// Two points that both continue to the same statement: a binding
    /// they leave differently is possibly null when either side says
    /// so, else back to its `flow_declared` type; a member narrow holds
    /// only when both sides hold it.
    pub(crate) fn join_flow(&self, a: &FlowState, b: &FlowState) -> FlowState {
        let other: HashMap<_, _> = b.bindings.iter().cloned().collect();
        let bindings = a
            .bindings
            .iter()
            .map(|(k, t)| {
                let joined = match other.get(k) {
                    None => t.clone(),
                    Some(u) if u == t => t.clone(),
                    Some(u) if matches!(u, Type::Nullable(_)) => u.clone(),
                    Some(u) => match (t, self.scopes.get(k.0).and_then(|s| s.get(&k.1))) {
                        (Type::Nullable(_), _) => t.clone(),
                        (
                            _,
                            Some(LocalInfo {
                                flow_declared: Some(d),
                                ..
                            }),
                        ) => d.clone(),
                        _ => u.clone(),
                    },
                };
                (k.clone(), joined)
            })
            .collect();
        let member = a
            .member
            .iter()
            .filter(|(k, t)| b.member.get(*k) == Some(*t))
            .map(|(k, t)| (k.clone(), t.clone()))
            .collect();
        FlowState { bindings, member }
    }

    /// The state at a point control reaches from anywhere inside
    /// `region`, which was entered at `entry` (the exit of a loop, a
    /// `switch` or a labeled block a `break` leaves, a `catch` or
    /// `finally` entry): the walk's state only describes the region's
    /// last statement. A binding possibly null at entry is so again,
    /// one the region writes is back to its `flow_declared` type, and
    /// a member narrow holds only when it held at entry and the region
    /// writes neither its receiver nor the member.
    pub(crate) fn join_region_exit(&mut self, ast: &Ast, entry: &FlowState, region: &[&Stmt]) {
        for ((d, n), t) in &entry.bindings {
            let Some(info) = self.scopes.get_mut(*d).and_then(|s| s.get_mut(n)) else {
                continue;
            };
            if let Some(declared) = &info.flow_declared {
                let hit = crate::check_assigns_to::binding_hit(ast, n);
                if region
                    .iter()
                    .any(|s| crate::check_assigns_to::writes(ast, &[], s, &hit))
                {
                    info.ty = declared.clone();
                    continue;
                }
            }
            if matches!(t, Type::Nullable(_)) && !matches!(info.ty, Type::Nullable(_)) {
                info.ty = t.clone();
            }
        }
        self.narrows.member.retain(|k, t| {
            entry.member.get(k) == Some(t)
                && region.iter().all(|s| {
                    crate::check_stmt_while::member_region(ast, s, k)
                        == crate::check_assigns_to::Region::NoWrite
                })
        });
    }

    /// Back to `before` for what `region` (walked from a wider state)
    /// did not write: a binding it writes keeps the walk's type, a
    /// member narrow holds when it held before and the region writes
    /// neither its receiver nor the member.
    pub(crate) fn resume_after(&mut self, ast: &Ast, before: &FlowState, region: &[&Stmt]) {
        for ((d, n), t) in &before.bindings {
            let hit = crate::check_assigns_to::binding_hit(ast, n);
            if region
                .iter()
                .any(|s| crate::check_assigns_to::writes(ast, &[], s, &hit))
            {
                continue;
            }
            if let Some(info) = self.scopes.get_mut(*d).and_then(|s| s.get_mut(n)) {
                info.ty = t.clone();
            }
        }
        self.narrows.member = before
            .member
            .iter()
            .filter(|(k, _)| {
                region.iter().all(|s| {
                    crate::check_stmt_while::member_region(ast, s, k)
                        == crate::check_assigns_to::Region::NoWrite
                })
            })
            .map(|(k, t)| (k.clone(), t.clone()))
            .collect();
    }
}
