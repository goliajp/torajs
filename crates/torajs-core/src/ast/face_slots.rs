//! The property and return slots some store hands a function value of
//! another face than the slot's declared type: `{ f: mk2(1) }` into a
//! `{ f: (y?: number) => number }` binding, `this.f = mk2(1)`, or
//! `return mk2(1)` from a function declared to return that type, where
//! `mk2` returns `(y?: number) => any`.
//!
//! A call through such a slot is shaped by the slot's face while the
//! value was compiled in its own, so the body read a boxed return as an
//! f64 (`o.f()` printed NaN, bun 6). The checker records the slot here
//! (`check::face_slots`); a call through it rides the boxed entry, whose
//! adapter unboxes per the stored value's own face, and the checker
//! types it by what the stored values return
//! (`check_type_of_call::fn_slot_ret`). Only a slot some store recorded
//! changes its calls; every other field and return keeps its direct
//! call.

use std::collections::{HashMap, HashSet};

use super::ExprId;

#[derive(Debug, Clone, Default)]
pub struct FaceSlots {
    /// (owner, field): the owner is the class that declares the field,
    /// or the structural spelling (`check::type_to_ann`) of an inline
    /// struct type, so literals of one shape share the verdict.
    pub fields: HashSet<(String, String)>,
    /// The functions that return such a value.
    pub rets: HashSet<String>,
    /// The calls through such a slot, a binding of the let-lane census
    /// or an `argv_boxed_params` parameter the checker typed `any`:
    /// their boxed answer is returned unconverted. Filled after
    /// checking.
    pub ret_any: HashSet<ExprId>,
    /// The object literals whose member stores the census was handed,
    /// each with the slot types it was handed them for: a literal
    /// missing here, or here only for another slot type, stores its
    /// members unrecorded.
    pub objlits: HashMap<ExprId, Vec<crate::check::Type>>,
}

impl FaceSlots {
    /// Does field `name` of a receiver owned by `owner` hold such a
    /// value? A class field is recorded under its declaring class, so a
    /// class receiver asks each of its ancestors too.
    pub fn field_hit(
        &self,
        class_parents: &HashMap<String, Option<String>>,
        owner: &str,
        name: &str,
    ) -> bool {
        if self.fields.is_empty() {
            return false;
        }
        let mut cur = Some(owner);
        while let Some(o) = cur {
            if self.fields.contains(&(o.to_string(), name.to_string())) {
                return true;
            }
            cur = class_parents.get(o).and_then(|p| p.as_deref());
        }
        false
    }
}
