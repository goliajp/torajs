//! The plan of an object slot that is not cast field by field (see
//! [`PlanTables::casts_into`]) and whose readers dispatch on the cell
//! itself: a value crosses as it is or not at all. The kernel
//! (`torajs-anyvalue::anyv_cast::as_is`) documents the encoding and
//! reads exactly what this writes.

use torajs_rc::Tag;

use super::anns::opt_node;
use super::{CastPlan, OPT, PlanTables, sid_tag};
use crate::ast::desugar_classes_builtin_heritage::{exotic_root_parent, is_typedarray_ctor};
use crate::check_type_ann::strip_nullable;
use crate::ssa::{StructId, Type};
use crate::ssa_lower_instanceof::tags::descendant_tags;

/// A cell of a builtin minted for a class of the list.
const SUB: u8 = 12;
/// A cell of a layout, or an instance of a class of the list.
const WIT: u8 = 13;

impl PlanTables<'_> {
    /// The interfaces dispatching through witness rows whose members
    /// are the fields of layout `sid`, in order.
    pub(super) fn iface_names(&self, sid: StructId) -> impl Iterator<Item = &str> {
        let layout = &self.struct_layouts[sid.0 as usize];
        (self.ast.iface_slots.layouts()).filter_map(move |(i, names)| {
            let same =
                names.len() == layout.len() && names.iter().zip(layout).all(|(n, (k, _))| *n == k);
            same.then_some(i)
        })
    }
}

/// The plan of an object slot of layout `sid` annotated `ann` that
/// [`PlanTables::casts_into`] does not cast field by field, for the
/// as-is kernel. `null` / `undefined` cross as the annotation admits
/// them, and only a cell the slot's readers can read:
///
/// - a class rooted in an exotic builtin: a cell of that builtin
///   minted for the class or a subclass (its instances are that
///   builtin's cells, which no layout test tells apart; a plain cell
///   of the builtin has none of the class's methods);
/// - an interface dispatching through witness rows: a cell of its
///   layout, or an instance of a class whose rows carry its witnesses.
///
/// `None` for any other object type with a method, whose slot takes a
/// cell of its layout through the plan [`super::compile`] writes.
pub(crate) fn uncast(t: &PlanTables<'_>, sid: StructId, ann: &str) -> Option<CastPlan> {
    let ty = Type::Obj(sid);
    let base = strip_nullable(ann.trim()).trim();
    let ifaces: Vec<&str> = t.iface_names(sid).collect();
    let exotic = (t.slot_class(base, sid))
        .then(|| exotic_root_parent(t.ast, base))
        .flatten();
    if exotic.is_none() && ifaces.is_empty() {
        return None;
    }
    let nullish = opt_node(ty, Some(ann));
    let mut bytes = vec![0; 4];
    if nullish {
        bytes.push(OPT);
    }
    let roots: Vec<&str> = match exotic {
        Some(_) => vec![base],
        None => (ifaces.iter())
            .flat_map(|i| t.ast.iface_slots.witnesses.get(*i).into_iter().flatten())
            .map(String::as_str)
            .collect(),
    };
    let mut kin: Vec<u32> = (roots.iter())
        .flat_map(|c| descendant_tags(&t.ast.class_parents, t.class_name_to_tag, c))
        .collect();
    kin.sort_unstable();
    kin.dedup();
    let (kind, tag) = match exotic {
        Some(parent) => (SUB, heap_tag(parent) as u32),
        None => (WIT, sid_tag(t, sid)),
    };
    bytes.push(kind);
    bytes.extend(tag.to_le_bytes());
    bytes.extend(0u32.to_le_bytes());
    let lists_off = bytes.len() as u32;
    bytes[..4].copy_from_slice(&lists_off.to_le_bytes());
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(crate::ssa_lower_boxed_entry::encode_class_list(&kin));
    Some(CastPlan {
        bytes,
        vtables: vec![None],
        nullish,
        members: false,
    })
}

/// The heap tag of the cells an exotic builtin `parent`'s subclass
/// mints (`exotic_alloc_self_magic` names the mint of each).
fn heap_tag(parent: &str) -> Tag {
    match parent {
        "Array" => Tag::Arr,
        "Number" => Tag::NumberWrapper,
        "String" => Tag::StringWrapper,
        "Boolean" => Tag::BooleanWrapper,
        "Function" => Tag::Closure,
        "Map" => Tag::Map,
        "Set" => Tag::Set,
        "Promise" => Tag::Promise,
        "RegExp" => Tag::RegExp,
        "WeakMap" => Tag::WeakMap,
        "WeakSet" => Tag::WeakSet,
        "Date" => Tag::Date,
        "WeakRef" => Tag::WeakRef,
        "FinalizationRegistry" => Tag::FinalizationRegistry,
        p if is_typedarray_ctor(p) => Tag::TypedArray,
        _ => unreachable!("not an exotic subclassable builtin: {parent}"),
    }
}
