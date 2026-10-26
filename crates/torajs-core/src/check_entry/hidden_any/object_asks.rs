//! What a round asks of the program's objects: the fields that may be
//! stored a value of any kind (`super::obj_fields`), and the methods
//! that may return one with every declaration sharing their return slot
//! (`super::method_rets`).

use std::collections::{HashMap, HashSet};

use super::{extend_own, method_lits, method_rets, obj_fields, obj_rets, samename};
use crate::ast::{Ast, ExprId, PropKey};
use crate::check::Type;

pub(super) struct ObjectAsks {
    obj_rets: obj_rets::ObjRets,
    field_stores: obj_fields::FieldStores,
    methods: method_rets::MethodRets,
    /// field names whose held fields are respelled `any`, with the
    /// receivers' classes they are stored through
    obj_fields: HashMap<PropKey, obj_fields::Owners>,
    /// methods found returning an `any` value
    method_found: HashSet<String>,
    /// the classes each widened field name leaves as they are
    /// (`super::samename`), as of the last round
    kept: HashMap<PropKey, HashSet<String>>,
}

impl ObjectAsks {
    pub(super) fn of(ast: &Ast) -> Self {
        ObjectAsks {
            obj_rets: obj_rets::ObjRets::of(ast),
            field_stores: obj_fields::FieldStores::of(ast),
            methods: method_rets::MethodRets::of(ast),
            obj_fields: HashMap::new(),
            method_found: HashSet::new(),
            kept: HashMap::new(),
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
        let names: HashSet<PropKey> = self.obj_fields.keys().cloned().collect();
        let mut lits = obj_fields::lit_values(ast, types, &names);
        let fields_grew = extend_fields(&mut self.obj_fields, self.field_stores.found(ast, types));
        let kept_shrank = self.settle_kept(ast, types);
        if !lits.is_empty() {
            let layouts = samename::layout_values(ast, &self.kept);
            lits.retain(|v| !layouts.contains(v));
        }
        let grew = (self.method_found.len() > before)
            | fields_grew
            | kept_shrank
            | extend_own(ast, fields, &lits);
        (found, grew)
    }

    /// Retype `r` for the asks taken so far, `types` being the last
    /// round's.
    pub(super) fn retype(&self, ast: &Ast, r: &mut Ast, types: &HashMap<ExprId, Type>) {
        let mut fam = (self.methods).family(ast, types, self.method_found.iter());
        let lits = method_lits::closures(ast, types, self.methods.sources(), &fam);
        fam.fns.extend(lits);
        method_rets::retype(r, &fam);
        (self.field_stores).retype(r, &self.obj_fields, &self.kept);
    }

    /// Take the classes each widened field name leaves as they are
    /// (`super::samename`) under a round's `types`; true when a class
    /// the last round left was taken out. A class taken out once stays
    /// out: its field was respelled `any` for the rounds after, whose
    /// types no longer show what took it out.
    fn settle_kept(&mut self, ast: &Ast, types: &HashMap<ExprId, Type>) -> bool {
        let last = std::mem::take(&mut self.kept);
        let mut shrank = last
            .keys()
            .any(|k| self.obj_fields.get(k).is_none_or(Option::is_none));
        for (k, o) in &self.obj_fields {
            let Some(owners) = o else {
                continue;
            };
            let mut cs = samename::kept(ast, types, owners);
            if let Some(was) = last.get(k) {
                cs.retain(|c| was.contains(c));
                shrank |= cs.len() < was.len();
            }
            self.kept.insert(k.clone(), cs);
        }
        shrank
    }
}

/// Add `found` to `asked`; true when a name or a receiver's class was
/// new.
fn extend_fields(
    asked: &mut HashMap<PropKey, obj_fields::Owners>,
    found: HashMap<PropKey, obj_fields::Owners>,
) -> bool {
    let mut grew = false;
    for (k, o) in found {
        let Some(at) = asked.get_mut(&k) else {
            asked.insert(k, o);
            grew = true;
            continue;
        };
        match (at.as_mut(), o) {
            (None, _) => {}
            (Some(_), None) => {
                *at = None;
                grew = true;
            }
            (Some(cs), Some(os)) => {
                let before = cs.len();
                cs.extend(os);
                grew |= cs.len() > before;
            }
        }
    }
    grew
}
