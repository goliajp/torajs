//! The cast plan of an object-layout slot: its static type, compiled
//! into the rodata bytes the object cast kernel walks
//! (`torajs-anyvalue::anyv_cast::plan` documents the encoding, and
//! reads exactly what this writes).
//!
//! A plan names a type the way the slot's readers see it: each field
//! of a layout with its own type (read off the annotation the program
//! wrote for it, which is what tells `T | null` and `T` apart and a
//! class from another type of the same fields), recursively. A type
//! that contains itself refers back to its own node. A field the
//! kernel cannot judge (a function, a substring view, a layout with
//! methods) leaves the slot without a plan: such a slot takes a cell
//! of its layout and nothing else.

use std::collections::HashMap;

use crate::ast::Ast;
use crate::check_type_ann::strip_nullable;
use crate::ssa::{StructId, Type};
use crate::ssa_lower_anon_stamp::AnonStampPoolCell;
use crate::ssa_lower_instanceof::tags::descendant_tags;
use anns::{field_anns, nullable, opt_node};

mod anns;

const ANY: u8 = 0;
const INT: u8 = 1;
const NUM: u8 = 2;
const BOOL: u8 = 3;
const STR: u8 = 4;
const BOXED: u8 = 5;
const OBJ: u8 = 6;
const REF: u8 = 7;
const ARR: u8 = 8;
const CELL: u8 = 9;
const OPT: u8 = 10;
const MEMBER: u8 = 11;
/// OBJ node flags: a class slot; a slot whose node copies nothing.
const CLASS: u8 = 1;
const FITS_ONLY: u8 = 2;
const NO_LIST: u32 = u32::MAX;

/// What a plan is compiled from.
pub(crate) struct PlanTables<'a> {
    pub(crate) ast: &'a Ast,
    pub(crate) struct_layouts: &'a [Vec<(crate::ast::PropKey, Type)>],
    pub(crate) arr_layouts: &'a [Type],
    pub(crate) class_name_to_tag: &'a HashMap<String, u32>,
    pub(crate) aliases: &'a HashMap<String, Type>,
    pub(crate) anon_stamp_pool: &'a AnonStampPoolCell,
}

impl PlanTables<'_> {
    /// Whether a slot of layout `sid` annotated `ann` is cast: a class
    /// slot unless its class descends from an exotic builtin; another
    /// object type unless it is an interface with witness rows or has
    /// a method (`super::obj`).
    pub(crate) fn casts_into(&self, sid: StructId, ann: &str) -> bool {
        let base = strip_nullable(ann.trim()).trim();
        if self.slot_class(base, sid) {
            // an instance of a class rooted in an exotic builtin is that
            // builtin's cell (a Promise, a Set), which no layout test
            // can tell apart; its slot keeps the value as it is
            return crate::ast::desugar_classes_builtin_heritage::exotic_root_parent(
                self.ast, base,
            )
            .is_none();
        }
        let layout = &self.struct_layouts[sid.0 as usize];
        let iface = self.ast.iface_slots.layouts().any(|(_, names)| {
            names.len() == layout.len() && names.iter().zip(layout).all(|(n, (k, _))| *n == k)
        });
        let methods = (layout.iter()).any(|(_, t)| matches!(t, Type::FnSig(_) | Type::Closure(_)));
        !(iface || methods)
    }

    /// Whether the annotation `base` names a declared class whose
    /// instances are laid out as `sid` (a generic instance has a layout
    /// of its own, and keeps the layout test).
    pub(crate) fn slot_class(&self, base: &str, sid: StructId) -> bool {
        self.ast.class_parents.contains_key(base)
            && self.class_name_to_tag.contains_key(base)
            && self.aliases.get(base) == Some(&Type::Obj(sid))
    }
}

/// A compiled plan: its bytes, and per object node the vtable global
/// its fresh cells wear (`None`: no vtable).
#[derive(Clone)]
pub(crate) struct CastPlan {
    pub(crate) bytes: Vec<u8>,
    pub(crate) vtables: Vec<Option<String>>,
    /// Some node admits `null` / `undefined` besides its type (a
    /// `T | null` or optional field the annotation spells).
    pub(crate) nullish: bool,
    /// Some object node has an optional member (`m?: T`) among its
    /// fields, which only the kernel's `_members` entry casts.
    pub(crate) members: bool,
}

/// The plan of a slot of type `ty` the program annotated `ann`. When
/// some field it reaches is one the kernel cannot judge, the plan
/// admits only a cell of the slot's layout: its object nodes copy
/// nothing.
pub(crate) fn compile(t: &PlanTables<'_>, ty: Type, ann: &str) -> CastPlan {
    build(t, ty, ann, false)
        .or_else(|| build(t, ty, ann, true))
        .expect("a plan that copies nothing judges no field")
}

fn build(t: &PlanTables<'_>, ty: Type, ann: &str, fits_only: bool) -> Option<CastPlan> {
    let mut b = Builder {
        t,
        fits_only,
        bytes: vec![0; 4],
        vtables: Vec::new(),
        lists: Vec::new(),
        open: HashMap::new(),
        nullish: false,
        members: false,
    };
    b.node(ty, Some(ann))?;
    let lists_off = b.bytes.len() as u32;
    b.bytes[..4].copy_from_slice(&lists_off.to_le_bytes());
    b.bytes.extend((b.lists.len() as u32).to_le_bytes());
    for list in &b.lists {
        b.bytes
            .extend(crate::ssa_lower_boxed_entry::encode_class_list(list));
    }
    Some(CastPlan {
        bytes: b.bytes,
        vtables: b.vtables,
        nullish: b.nullish,
        members: b.members,
    })
}

/// The class lists of `plan` (the bytes [`compile`] wrote), each
/// widened by `admit`, written back in place.
pub(crate) fn rewrite_class_lists(plan: &mut Vec<u8>, admit: &dyn Fn(&mut Vec<u32>)) {
    let word = |b: &[u8], at: usize| u32::from_le_bytes(b[at..at + 4].try_into().unwrap());
    let lists_off = word(plan, 0) as usize;
    let mut at = lists_off + 4;
    let mut tail = plan[lists_off..lists_off + 4].to_vec();
    for _ in 0..word(plan, lists_off) {
        let n = word(plan, at) as usize;
        let mut tags: Vec<u32> = (0..n).map(|i| word(plan, at + 4 + 4 * i)).collect();
        at += 4 + 4 * n;
        admit(&mut tags);
        tail.extend(crate::ssa_lower_boxed_entry::encode_class_list(&tags));
    }
    plan.truncate(lists_off);
    plan.extend(tail);
}

struct Builder<'t, 'a> {
    t: &'t PlanTables<'a>,
    /// Object nodes admit only a cell of their layout.
    fits_only: bool,
    bytes: Vec<u8>,
    vtables: Vec<Option<String>>,
    lists: Vec<Vec<u32>>,
    /// The OBJ node already written for a layout and its class.
    open: HashMap<(StructId, Option<String>), u32>,
    nullish: bool,
    members: bool,
}

impl Builder<'_, '_> {
    fn node(&mut self, ty: Type, ann: Option<&str>) -> Option<()> {
        let nullable = nullable(ann);
        let base = ann.map(|a| strip_nullable(a.trim()).trim());
        if opt_node(ty, ann) {
            self.bytes.push(OPT);
            self.nullish = true;
        }
        self.nullish |= nullable && ty == Type::Any && matches!(base, Some("number" | "boolean"));
        match ty {
            Type::Any if nullable && base == Some("number") => self.bytes.extend([BOXED, 0]),
            Type::Any if nullable && base == Some("boolean") => self.bytes.extend([BOXED, 1]),
            Type::Any => self.bytes.push(ANY),
            Type::I64 | Type::I32 => self.bytes.push(INT),
            Type::F64 => self.bytes.push(NUM),
            Type::Bool => self.bytes.push(BOOL),
            Type::Str => self.bytes.push(STR),
            Type::Obj(sid) => self.obj(sid, base)?,
            Type::Arr(id) => {
                let elem = self.t.arr_layouts[id.0 as usize];
                let kind = crate::ssa_lower_stmt_let_decl_convert::arr_elem_kind_const(&elem);
                self.bytes.extend([ARR, kind.unwrap_or(0) as u8]);
                let elem_ann = base.and_then(crate::ast_throw_info::elem_ann);
                self.node(elem, elem_ann.as_deref())?;
            }
            _ => {
                let tag = cell_tag(ty)?;
                self.bytes.push(CELL);
                self.bytes.extend((tag as u16).to_le_bytes());
            }
        }
        Some(())
    }

    fn obj(&mut self, sid: StructId, base: Option<&str>) -> Option<()> {
        let class = base
            .filter(|b| self.t.slot_class(b, sid))
            .map(str::to_string);
        if let Some(&at) = self.open.get(&(sid, class.clone())) {
            self.bytes.push(REF);
            self.bytes.extend(at.to_le_bytes());
            return Some(());
        }
        let mut layout = self.t.struct_layouts[sid.0 as usize].clone();
        if !self.fits_only
            && class.is_none()
            && layout
                .iter()
                .any(|(_, t)| matches!(t, Type::FnSig(_) | Type::Closure(_)))
        {
            return None;
        }
        let mut flags = if class.is_some() { CLASS } else { 0 };
        if self.fits_only {
            flags |= FITS_ONLY;
            layout.clear();
        }
        let (anns, optional) = field_anns(self.t.ast, class.as_deref().or(base)).unzip();
        let (tag, list, vtable) = match &class {
            Some(c) => {
                let tags = descendant_tags(&self.t.ast.class_parents, self.t.class_name_to_tag, c);
                self.lists.push(tags);
                // a program with no virtual method and no interface slot
                // has no vtables (`ssa_lower_object_lit::alloc`)
                let ast = self.t.ast;
                let vt = (!(ast.method_index.is_empty() && ast.iface_slots.is_empty()))
                    .then(|| format!("__vtable_{c}"));
                (self.t.class_name_to_tag[c], self.lists.len() as u32 - 1, vt)
            }
            None => (self.sid_class_tag(sid), NO_LIST, self.lit_vtable(sid)),
        };
        let at = self.bytes.len() as u32;
        self.open.insert((sid, class.clone()), at);
        self.bytes.push(OBJ);
        self.bytes.push(flags);
        self.bytes.extend(tag.to_le_bytes());
        self.bytes.extend(list.to_le_bytes());
        self.bytes.extend((self.vtables.len() as u32).to_le_bytes());
        self.vtables.push(vtable);
        self.bytes.extend((layout.len() as u16).to_le_bytes());
        for (i, (key, fty)) in layout.iter().enumerate() {
            let start = self.bytes.len();
            self.bytes.extend([0; 4]);
            let off = crate::ssa_lower::OBJ_HEADER_SIZE as u32 + (i as u32) * 8;
            self.bytes.extend(off.to_le_bytes());
            let name = key.as_wtf8().as_bytes();
            self.bytes.extend((name.len() as u16).to_le_bytes());
            self.bytes.extend(name);
            let fann = anns.as_ref().and_then(|m| m.get(name).cloned());
            // an optional member's node: a value lacking the key leaves
            // it out of the copy
            if optional.as_ref().is_some_and(|o| o.contains(name))
                && opt_node(*fty, fann.as_deref())
            {
                self.bytes.push(MEMBER);
                self.members = true;
            }
            self.node(*fty, fann.as_deref())?;
            let end = self.bytes.len() as u32;
            self.bytes[start..start + 4].copy_from_slice(&end.to_le_bytes());
        }
        Some(())
    }

    /// The tag a cell of layout `sid` wears when no class is named:
    /// the lowest of the classes laid out as `sid`, else its anonymous
    /// stamp (the split the object literal alloc stamps by).
    fn sid_class_tag(&self, sid: StructId) -> u32 {
        (self.t.class_name_to_tag.iter())
            .filter(|(name, _)| self.t.aliases.get(*name) == Some(&Type::Obj(sid)))
            .map(|(_, &tag)| tag)
            .min()
            .unwrap_or_else(|| self.t.anon_stamp_pool.borrow_mut().assign_or_get(sid))
    }

    /// The literal vtable row of layout `sid`, when an interface slot
    /// may read such a value (`ssa_lower_iface::store_lit_vtable`).
    fn lit_vtable(&self, sid: StructId) -> Option<String> {
        let layout = &self.t.struct_layouts[sid.0 as usize];
        (!self.t.ast.iface_slots.is_empty()
            && !crate::ssa_lower_iface::lit_row_ifaces(self.t.ast, layout).is_empty())
        .then(|| format!("__vtable_{}", crate::ssa_lower_iface::lit_row_name(sid)))
    }
}

/// The heap tag every value of the single-family heap type `ty` wears.
fn cell_tag(ty: Type) -> Option<torajs_rc::Tag> {
    use torajs_rc::Tag;
    Some(match ty {
        Type::Closure(_) => Tag::Closure,
        Type::Map => Tag::Map,
        Type::Set => Tag::Set,
        Type::Date => Tag::Date,
        Type::RegExp => Tag::RegExp,
        Type::Promise => Tag::Promise,
        Type::Symbol => Tag::Symbol,
        Type::BigInt => Tag::BigInt,
        Type::WeakRef => Tag::WeakRef,
        Type::WeakMap => Tag::WeakMap,
        Type::WeakSet => Tag::WeakSet,
        _ => return None,
    })
}
