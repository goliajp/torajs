//! Facts the lowering establishes while it emits and spends later in
//! the same fn.
//!
//! - A binding whose value passed a nullish guard holds a live cell
//!   from that point on, until something writes it: the member reads
//!   after the first one on `const t = xs[i]` need no second test.
//!   The fact is flow-sensitive and kept conservatively — a nested
//!   region (branch arm, loop, try, switch clause) returns only the
//!   facts that held before it, minus any it killed; a loop forgets
//!   everything at entry (a body write reaches the next iteration's
//!   reads); a sub-expression that runs on one branch of a join
//!   records nothing.
//! - An expression the producing lane proved non-nullish (a fused
//!   element read that throws where the plain read answers
//!   `undefined`).
//! - The whole-program answer to "can any array carry a shifted deque
//!   head", from [`crate::ssa_lower_pass_0_5::arr_head_census`].
//! - The receivers of a constructor's leading `this.f = v` writes,
//!   where the instance the factory just minted has not escaped yet
//!   (`ssa_lower_fn::fresh_this`): nothing can have frozen it or
//!   redefined a field, so the store needs no writable guard, and a
//!   field's first write finds the factory's nullish default.

use std::collections::{HashMap, HashSet};

use crate::ast::ExprId;
use crate::ssa_lower_pass_0_5::value_class_census::FieldInit as ValueFieldInit;

/// What the whole-program censuses of pass 0.5 established, shared by
/// every fn's proofs.
#[derive(Default)]
pub(crate) struct ProgramFacts {
    /// No array in the program ever has a non-zero deque head
    /// (`ssa_lower_pass_0_5::arr_head_census`).
    pub(crate) arr_head_pinned: bool,
    /// `(class, field)` pairs whose reads never see the undefined cell
    /// (`ssa_lower_pass_0_5::live_field_census`).
    pub(crate) live_fields: crate::ssa_lower_pass_0_5::live_field_census::LiveFields,
    /// A typed array's read at a non-integral key can run user code
    /// (`compute_may_throw_fns`): a getter or a `valueOf` reachable
    /// through the property the key names.
    pub(crate) nonint_reads_run_code: bool,
    /// Classes no code can tell apart from a copy of their fields
    /// (`ssa_lower_pass_0_5::value_class_census`).
    pub(crate) value_classes: HashSet<crate::ssa::StructId>,
    /// What each field of a new instance of a value class holds.
    pub(crate) value_inits: HashMap<crate::ssa::StructId, Vec<(String, ValueFieldInit)>>,
    /// No receiver can enter a method twin (`__cmany_<C>__<m>`) the
    /// censuses left out (`arr_head_census::unreachable_twin_exprs`).
    pub(crate) twins_unreachable: bool,
    /// Every name some statement of the program assigns, read once on
    /// first need ([`Proofs::assigned_anywhere`]).
    pub(crate) assigned: std::cell::OnceCell<HashSet<String>>,
}

pub(crate) type ProgramFactsRc = std::rc::Rc<ProgramFacts>;

#[derive(Default)]
pub(crate) struct Proofs {
    /// The whole-program facts of pass 0.5.
    facts: ProgramFactsRc,
    /// Index reads that are the receiver of a member read of the named
    /// field; the typed lane fuses the bounds / hole exits into the
    /// TypeError that read would raise anyway.
    pub(crate) recv_field: HashMap<ExprId, String>,
    /// Expressions the lane that produced them proved non-nullish.
    pub(crate) nonnull_exprs: HashSet<ExprId>,
    /// `this` receivers of a ctor's field writes reached before the
    /// instance escapes: unfrozen, no field redefined.
    pub(crate) fresh_this_recv: HashSet<ExprId>,
    /// The subset whose field holds the factory's null / undefined
    /// default at that point, so there is no old value to release.
    pub(crate) fresh_this_first_write: HashSet<ExprId>,
    /// Inits of borrow-shaped lets whose binding is read after
    /// something may have released its source
    /// (`crate::ssa_lower::alias_hazard`): they bind owned.
    pub(crate) released_alias_inits: HashSet<ExprId>,
    /// The same, with an append counted too, filled only when some
    /// class has its arrays held in place: an inline element read
    /// bound here could see its array's buffer move
    /// (`crate::ssa_lower::inline_elem::assert_view_binding`).
    pub(crate) moved_view_inits: HashSet<ExprId>,
    /// Element stores `xs[i] = s` that may take the `const` binding's
    /// own stake: the rest of its block only reads `s`
    /// (`crate::ssa_lower_index_assign::move_const_temp`).
    pub(crate) slot_moves: HashMap<ExprId, String>,
    /// Fn-local bindings whose current value passed a nullish guard on
    /// every path reaching the point being lowered.
    nonnull_lets: HashSet<String>,
    /// > 0 while lowering a sub-expression that runs on one branch of
    /// > a join (`a && b`, `c ? x : y`, `o?.p`, `a ?? b`).
    cond_depth: u32,
    /// The drop being emitted releases a binding a guard proved live,
    /// so its nullish test is known false (`emit_frame_drops`).
    drop_live: bool,
}

impl Proofs {
    pub(crate) fn new(facts: &ProgramFactsRc) -> Self {
        Proofs {
            facts: facts.clone(),
            ..Default::default()
        }
    }

    /// No array in the program ever has a non-zero deque head, so
    /// every element lives at `data + idx * stride`.
    pub(crate) fn arr_head_pinned(&self) -> bool {
        self.facts.arr_head_pinned
    }

    /// What each field of a new instance of the value class `sid`
    /// holds, by field name.
    pub(crate) fn value_inits(&self, sid: crate::ssa::StructId) -> &[(String, ValueFieldInit)] {
        self.facts.value_inits.get(&sid).map_or(&[], Vec::as_slice)
    }

    /// Every name some statement of the program assigns (`=`, `++`),
    /// in any function.
    pub(crate) fn assigned_anywhere(&self, ast: &crate::ast::Ast) -> &HashSet<String> {
        self.facts.assigned.get_or_init(|| {
            crate::ssa_lower_closure_captures::collect_assigned_names_scoped(
                ast,
                ast.stmts.iter(),
                &[],
            )
        })
    }

    /// See [`ProgramFacts::nonint_reads_run_code`].
    pub(crate) fn nonint_reads_run_code(&self) -> bool {
        self.facts.nonint_reads_run_code
    }

    /// `obj.name` reads a field the census proved live.
    pub(crate) fn field_is_live(&self, recv: Option<&crate::check::Type>, name: &str) -> bool {
        let Some(crate::check::Type::ClassRef(c)) = recv else {
            return false;
        };
        let c = c.split('<').next().unwrap_or(c);
        self.facts
            .live_fields
            .contains(&(c.to_string(), name.to_string()))
    }

    pub(crate) fn let_is_nonnull(&self, name: &str) -> bool {
        self.nonnull_lets.contains(name)
    }

    /// A guard on `name` just ran on the straight-line path.
    pub(crate) fn note_let_nonnull(&mut self, name: &str) {
        if self.cond_depth == 0 {
            self.nonnull_lets.insert(name.to_string());
        }
    }

    /// `name` is (re)bound or written: whatever was proven about its
    /// old value is gone.
    pub(crate) fn kill(&mut self, name: &str) {
        self.nonnull_lets.remove(name);
    }

    /// Loop entry: a write anywhere in the loop reaches the reads
    /// before it on the next iteration.
    pub(crate) fn forget_lets(&mut self) {
        self.nonnull_lets.clear();
    }

    /// Snapshot before a nested region; pair with [`Self::region_leave`].
    pub(crate) fn region_enter(&self) -> HashSet<String> {
        self.nonnull_lets.clone()
    }

    /// Back on the path around the region: keep a fact only if it held
    /// before the region and the region did not kill it.
    pub(crate) fn region_leave(&mut self, saved: &HashSet<String>) {
        self.nonnull_lets.retain(|n| saved.contains(n));
    }

    /// The next drop releases a binding a guard proved live.
    pub(crate) fn arm_drop_live(&mut self) {
        self.drop_live = true;
    }

    /// Whether the drop asking is the one [`Self::arm_drop_live`] armed;
    /// it disarms, so the drops of the value's own fields test theirs.
    pub(crate) fn take_drop_live(&mut self) -> bool {
        std::mem::take(&mut self.drop_live)
    }

    pub(crate) fn enter_cond(&mut self) {
        self.cond_depth += 1;
    }

    pub(crate) fn leave_cond(&mut self) {
        self.cond_depth -= 1;
    }
}
