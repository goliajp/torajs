//! What the shadow census (`super::shadow_names`) leaves on the AST,
//! and the answers the lowering reads off it.

use std::collections::{HashMap, HashSet};

use super::super::{Ast, ExprId, PropKey};
use super::accessor_names::{AccessorFieldNames, Reach};

/// The shadow census's verdicts, settled after checking.
#[derive(Debug, Clone, Default)]
pub struct ShadowFacts {
    /// The fields a write of the program can park in the sidecar: a
    /// value the layout slot has no spelling for, written through the
    /// any lane (`super::shadow_names`). A typed read of one tests the
    /// same header bit as an accessor's and answers the sidecar.
    pub(super) shadow: Reach,
    /// The fields a `number` the program writes through the any lane
    /// can reach: it fits an f64 slot, and shadows any other.
    pub(super) shadow_number: Reach,
    /// A cast from the any lane into a layout with an optional member
    /// can build a cell lacking it (`super::cast_targets`), so the
    /// absent set is in play even when no literal leaves a member out.
    pub cast_leaves_members_out: bool,
    /// The optional `number` members whose slot is an f64
    /// (`super::optional_number_fields`): a literal
    /// stores a value such a slot cannot spell through the any lane.
    pub(super) optional_number: HashSet<PropKey>,
    /// The members whose typed reads the checker typed `any`: an
    /// any-lane write can leave them holding a value of another type
    /// (`check_type_of_member::mistyped_read`).
    pub(super) mistyped: super::shadow_names::ShadowedNames,
}

impl Reach {
    pub(super) fn may(&self, name: &str) -> bool {
        match self {
            Reach::None => false,
            Reach::Names(n) => n.contains(name),
            Reach::All => true,
        }
    }
}

impl AccessorFieldNames {
    /// Whether `name` is an optional `number` member kept in an f64 slot.
    pub fn optional_number_member(&self, name: &PropKey) -> bool {
        self.shadowed.optional_number.contains(name)
    }

    /// Every optional `number` member kept in an f64 slot.
    pub fn optional_number_members(&self) -> &HashSet<PropKey> {
        &self.shadowed.optional_number
    }

    /// Whether the checker typed the reads of the member `name`, whose
    /// slot is a `slot` one, `any` for a value of another type an
    /// any-lane write can leave there.
    pub fn mistyped_read(&self, name: &str, slot: super::shadow_names::SlotKind) -> bool {
        self.shadowed.mistyped.may_mistype(name, slot)
    }

    /// Whether a write of the program can leave the declared field
    /// `name` holding a value its slot cannot spell; `f64_slot` says
    /// the slot holds every number, which a `number` write then fits.
    pub fn may_shadow(&self, name: &str, f64_slot: bool) -> bool {
        self.shadowed.shadow.may(name) || (!f64_slot && self.shadowed.shadow_number.may(name))
    }

    /// [`AccessorFieldNames::may`] as it will answer once
    /// [`AccessorFieldNames::settle_key_reads`] has run over
    /// `expr_types`, for a pass that runs before.
    pub(super) fn may_settled(
        &self,
        ast: &Ast,
        expr_types: &HashMap<ExprId, crate::check::Type>,
    ) -> impl Fn(&str) -> bool + use<'_> {
        let open = self.key_reads_open(ast, expr_types);
        move |name: &str| open || self.reach.may(name)
    }
}
