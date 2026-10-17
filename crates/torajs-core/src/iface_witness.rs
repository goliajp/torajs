//! Interface witness rows.
//!
//! An interface (any non-class, non-generic object type declaration) is
//! a structural type: a class instance whose members match it is one of
//! its values, as is an object literal of its shape. Both are the same
//! object pointer at run time, but a class lays its fields out in its
//! own order and keeps its methods out of the instance, so a member
//! access through the interface cannot read the interface's own field
//! offsets once a class instance can be behind it.
//!
//! Such an interface gets one vtable slot per member face — a call face
//! for a method, a get and a set face for a data member — appended after
//! the class method slots. Every class that satisfies it fills those
//! slots of its row with witness thunks (`__iw_<C>__<I>__<face>`), each
//! forwarding to the class's own method or field through the ordinary
//! typed lanes, and every object-literal layout that can stand in for
//! it carries a row of `__ilit_<I>__<face>` thunks reading its own
//! fields. Swift's protocol witness tables have this shape; the slot is
//! fixed per (interface, face) program-wide, so a call through the
//! interface is a vtable load and an indirect call, the same as a
//! virtual method.
//!
//! The rows are planned for every interface some class satisfies, but
//! an interface keeps its slots only when the checker saw a class
//! instance flow into it (`settle`) and one such class cannot be read
//! at the interface's own offsets: a method, a member the class lacks,
//! or a field laid out elsewhere. Every other interface keeps the
//! plain field loads it has always had, and its thunks are dropped.

mod admit;
mod body;
mod inline;
mod plan;
mod settle;
mod widen;

pub(crate) use admit::{begin_compile, class_admit, install, note_flow};
pub(crate) use settle::settle;

use std::collections::{BTreeSet, HashMap};

/// The interfaces that dispatch through witness slots, and their faces.
#[derive(Debug, Clone, Default)]
pub struct IfaceSlots {
    /// `(interface, face)` in slot order; slot `i` of the table is
    /// `method_index.len() + i`.
    pub faces: Vec<(String, String)>,
    /// interface → the classes whose rows carry its witness thunks.
    pub witnesses: HashMap<String, BTreeSet<String>>,
    /// interface → its resolved type, which is what a receiver typed by
    /// it checks as (a value of it may sit on more than one layout id)
    pub types: HashMap<String, crate::check::Type>,
    /// the interfaces that declare a method
    pub with_methods: BTreeSet<String>,
    /// interface → the classes whose instances read right at the
    /// interface's own field offsets (its fields first, in order)
    pub plain: HashMap<String, BTreeSet<String>>,
    /// interface → the names of its witness thunks
    pub thunks: HashMap<String, Vec<String>>,
    /// interface → the classes whose rows read some member through
    /// the instance's dynamic properties because the class declares it
    /// with another type: the structural rules, not the row, decide
    /// whether such a class fits
    pub loose: HashMap<String, BTreeSet<String>>,
}

/// What a face does with its member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaceKind {
    Call,
    Get,
    Set,
    /// whether an optional method is present (what `recv.m?.()` tests)
    Has,
}

impl IfaceSlots {
    /// The interface a receiver checked as `ty` dispatches through —
    /// the first by name when two declare one shape.
    pub fn iface_of(&self, ty: &crate::check::Type) -> Option<&str> {
        let mut hit: Option<&str> = None;
        for (i, t) in &self.types {
            if t == ty && hit.is_none_or(|h| i.as_str() < h) {
                hit = Some(i);
            }
        }
        hit
    }

    /// The member names of every interface, in declaration order.
    pub fn layouts(&self) -> impl Iterator<Item = (&str, Vec<&crate::ast::PropKey>)> {
        self.types.iter().filter_map(|(i, t)| match t {
            crate::check::Type::Struct(fs) => {
                Some((i.as_str(), fs.iter().map(|(k, _)| k).collect()))
            }
            _ => None,
        })
    }

    /// The slot index of `iface`'s `face`, counted after the
    /// `base` method slots.
    pub fn slot_of(&self, base: usize, iface: &str, face: &str) -> Option<u32> {
        self.faces
            .iter()
            .position(|(i, f)| i == iface && f == face)
            .map(|p| (base + p) as u32)
    }

    pub fn is_empty(&self) -> bool {
        self.faces.is_empty()
    }
}

pub fn get_face(key: &str) -> String {
    format!("{key}__get")
}

pub fn set_face(key: &str) -> String {
    format!("{key}__set")
}

pub fn has_face(key: &str) -> String {
    format!("{key}__has")
}

/// The member a face reads and what it does with it.
pub fn face_kind(face: &str) -> (&str, FaceKind) {
    if let Some(k) = face.strip_suffix("__get") {
        (k, FaceKind::Get)
    } else if let Some(k) = face.strip_suffix("__set") {
        (k, FaceKind::Set)
    } else if let Some(k) = face.strip_suffix("__has") {
        (k, FaceKind::Has)
    } else {
        (face, FaceKind::Call)
    }
}

pub fn class_thunk(class: &str, iface: &str, face: &str) -> String {
    format!("__iw_{class}__{iface}__{face}")
}

pub fn lit_thunk(iface: &str, face: &str) -> String {
    format!("__ilit_{iface}__{face}")
}

/// Synthesize every witness thunk and record the slot table. Runs after
/// the class method signatures are final (implicit generics and the
/// vtable slot joins), before the default / rest argument passes, which
/// then treat the thunks' forwarding calls like any other call.
pub fn synthesize_iface_witnesses(ast: &mut crate::ast::Ast) {
    inline::synthesize(ast);
    let mut plans = plan::plan(ast);
    if widen::widen_written(ast, &plans) {
        plans = plan::plan(ast);
    }
    let mut slots = IfaceSlots::default();
    if plans.is_empty() {
        ast.iface_slots = slots;
        return;
    }
    let tables = crate::check_pipeline::collect_type_alias_tables(ast);
    let mut appended = Vec::new();
    for p in plans {
        for (face, _, _) in &p.lit_faces {
            slots.faces.push((p.iface.clone(), face.clone()));
        }
        let from = appended.len();
        body::emit_iface(ast, &p, &mut appended);
        let names = appended[from..].iter().filter_map(|s| match s {
            crate::ast::Stmt::FnDecl { name, .. } => Some(name.clone()),
            _ => None,
        });
        slots.thunks.insert(p.iface.clone(), names.collect());
        slots
            .witnesses
            .insert(p.iface.clone(), p.classes.keys().cloned().collect());
        if let Some(t) = tables.aliases.get(&p.iface) {
            slots.types.insert(p.iface.clone(), t.clone());
        }
        if p.with_method {
            slots.with_methods.insert(p.iface.clone());
        }
        slots.plain.insert(p.iface.clone(), p.plain);
        slots.loose.insert(p.iface.clone(), p.loose);
    }
    slots.faces.sort();
    ast.stmts.extend(appended);
    ast.iface_slots = slots;
}
