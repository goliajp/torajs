//! What a round asks of the program's objects: the fields that may be
//! stored a value of any kind (`super::obj_fields`), and the methods
//! that may return one with every declaration sharing their return slot
//! (`super::method_rets`).

use std::collections::{HashMap, HashSet};

use super::{extend_own, method_lits, method_rets, obj_fields, obj_rets};
use crate::ast::{Ast, ExprId, PropKey};
use crate::check::Type;

pub(super) struct ObjectAsks {
    obj_rets: obj_rets::ObjRets,
    field_stores: obj_fields::FieldStores,
    methods: method_rets::MethodRets,
    /// field names whose held fields are respelled `any`
    obj_fields: HashSet<PropKey>,
    /// methods found returning an `any` value
    method_found: HashSet<String>,
}

impl ObjectAsks {
    pub(super) fn of(ast: &Ast) -> Self {
        ObjectAsks {
            obj_rets: obj_rets::ObjRets::of(ast),
            field_stores: obj_fields::FieldStores::of(ast),
            methods: method_rets::MethodRets::of(ast),
            obj_fields: HashSet::new(),
            method_found: HashSet::new(),
        }
    }

    /// Take a round's answers (`types`, and the functions `rets` the
    /// width pass finds returning a value of any kind): the functions
    /// found returning one, and whether an ask grew. Object literal
    /// values that a widened field name holds go to `fields`.
    pub(super) fn extend(
        &mut self,
        ast: &Ast,
        types: &HashMap<ExprId, Type>,
        rets: &HashSet<String>,
        fields: &mut HashSet<ExprId>,
    ) -> (Vec<String>, bool) {
        let obj = self.obj_rets.found(types);
        let found: Vec<String> = rets.iter().chain(&obj).cloned().collect();
        let before = self.method_found.len();
        (self.method_found).extend(found.iter().filter(|f| self.methods.holds(f)).cloned());
        let lits = obj_fields::lit_values(ast, types, &self.obj_fields);
        let grew = (self.method_found.len() > before)
            | extend_fields(&mut self.obj_fields, self.field_stores.found(ast, types))
            | extend_own(ast, fields, &lits);
        (found, grew)
    }

    /// Retype `r` for the asks taken so far, `types` being the last
    /// round's.
    pub(super) fn retype(&self, ast: &Ast, r: &mut Ast, types: &HashMap<ExprId, Type>) {
        let mut fam = (self.methods).family(ast, types, self.method_found.iter());
        let lits = method_lits::closures(ast, types, &fam);
        fam.fns.extend(lits);
        method_rets::retype(r, &fam);
        self.field_stores.retype(r, &self.obj_fields);
    }
}

/// Add `found` to `asked`; true when one was new.
fn extend_fields(asked: &mut HashSet<PropKey>, found: HashSet<PropKey>) -> bool {
    let before = asked.len();
    asked.extend(found);
    asked.len() > before
}
