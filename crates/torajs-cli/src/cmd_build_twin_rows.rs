//! Which class-method rows keep their `__cmany_` twin column.
//!
//! A row's twin is read by exactly one runtime entry,
//! `__torajs_struct_method_twin_at`, which mints the reified method
//! faces of a class's prototype; a face routes a re-bound foreign
//! receiver to the twin. A prototype is reachable only from a value of
//! the any world: an instance of the class (or of a class whose
//! prototype chain runs through it) through `Object.getPrototypeOf` /
//! `constructor` / an inherited member read, or the class or prototype
//! cell itself. So a class none of whose instances can leave the typed
//! lanes ([`crate::cmd_build_value_escape`]), in a program whose class
//! cells stay inside their prologue and whose code takes none of the
//! other roads to a prototype ([`KEEP_ALL_PREFIXES`]), never has a face
//! minted: its twins are never read, and its rows
//! point at a loud-reject stub instead ([`twin_replacements`]). A
//! twin nothing roots is then neither linked nor scanned by the
//! dispatch judgment, whose any-lane member reads (every twin reads
//! its receiver through the any lane) would otherwise keep the whole
//! dispatch world alive.
//!
//! An instance of class `C` may sit in a value of any struct type
//! whose fields are a prefix of `C`'s — the only views of `C`'s
//! layout that read `C`'s fields at their own offsets — so an escaping
//! value of such a type keeps `C`'s twins. A program with a user class
//! hierarchy (`__torajs_proto_link_fresh` links a subclass prototype
//! to its parent's) keeps every twin as soon as any instance escapes.

use torajs_codegen::CompiledFunction;
use torajs_codegen::frame::FrameLayout;
use torajs_core::ast::PropKey;
use torajs_core::ssa::{ClassLayoutMeta, InstKind, Module, Type};

/// Calls after which some prototype may be reachable without any
/// instance escaping in a way the walk follows: the class registry's
/// readers, class evaluation units, the global object and eval, and
/// the roads the drop proofs also treat as unseen (Reflect, Proxy, an
/// arguments object, `with`, a script, a constructor as a value).
const KEEP_ALL_PREFIXES: [&str; 20] = [
    "__torajs_anyv_class_get",
    "__torajs_anyv_proto_get",
    "__torajs_obj_proto_borrowed",
    "__torajs_class_source_for_cell",
    "__torajs_class_kind_derived",
    "__torajs_class_default_proto",
    "__torajs_class_unit",
    "__torajs_unit_",
    "__torajs_globalthis_object",
    "__torajs_global_eval",
    "__torajs_reflect_",
    "__torajs_anyv_reflect_",
    "__torajs_ns_object_reflect",
    "__torajs_proxy",
    "__torajs_arguments",
    "__torajs_body_arguments",
    "__torajs_with_",
    "__torajs_script_",
    "__torajs_builtin_ctor_value",
    "__torajs_anyv_construct",
];

/// The subclass prototype link: a user class hierarchy.
const HIERARCHY: &str = "__torajs_proto_link_fresh";

/// The runtime kernels an instance may be handed to without leaving
/// the typed lanes: refcount and release traffic (the cycle
/// collector's candidate buffer, the weak registry's death notice and
/// the expando bag's release free cells, never hand one out), the
/// null check.
fn keeps_instance(name: &str) -> bool {
    matches!(
        name,
        "__torajs_rc_inc"
            | "__torajs_rc_dec"
            | "__torajs_obj_drop_sized"
            | "__torajs_drop_release"
            | "__torajs_cycle_buffer"
            | "__torajs_weakref_target_dying"
            | "__torajs_obj_props_drop_slow"
            | "__torajs_heap_nullish_check"
    )
}

/// The reject pad's fam id for a dropped twin column: the TypeError
/// names a "class method twin".
const TWIN_REJECT_FAM: u32 = 59;

/// The stub a dropped twin column points at. The dot keeps it apart
/// from every user fn name, and it is not `___torajs_`-shaped, so only
/// a row that points at it roots it for the link.
const TWIN_REJECT_SYM: &str = "__torajs.twin_stripped";

/// Per `module.class_layouts` row: `None` while its twin column stays,
/// else the fn index its twin slots bake instead. A wrong judgment
/// then fails loud: a face routing a foreign receiver to the twin
/// lands on a stub that throws the named TypeError, where a 0 would
/// run the mono body over the foreign receiver's layout. The stub is
/// appended to `funcs` once, when some row needs it.
pub(crate) fn twin_replacements(
    module: &Module,
    funcs: &mut Vec<CompiledFunction>,
) -> Vec<Option<u32>> {
    let kept = if strip_all_forced() {
        vec![false; module.class_layouts.len()]
    } else {
        twin_rows_kept(module)
    };
    replacements(&kept, funcs)
}

/// [`twin_replacements`] for a decided `kept`.
pub(crate) fn replacements(kept: &[bool], funcs: &mut Vec<CompiledFunction>) -> Vec<Option<u32>> {
    if kept.iter().all(|k| *k) {
        return vec![None; kept.len()];
    }
    let stub = funcs.len() as u32;
    let (bytes, relocs) = crate::cmd_build_dispatch_stubs::reject_stub_body(TWIN_REJECT_FAM);
    funcs.push(CompiledFunction {
        name: TWIN_REJECT_SYM.into(),
        bytes,
        relocs,
        frame: FrameLayout::leaf_no_spill(),
    });
    kept.iter().map(|k| (!k).then_some(stub)).collect()
}

/// Diagnosis override — drops every twin column onto the reject stub,
/// to exercise it in a program whose faces do route to a twin.
fn strip_all_forced() -> bool {
    std::env::var_os("TORAJS_TWIN_ROWS_STRIP_ALL").is_some_and(|v| v != "0")
}

/// Per `module.class_layouts` row: whether its twin column stays.
///
/// The escape walk runs first: when it alone keeps every twin, nothing
/// else can drop one, and the program-wide scans are skipped.
pub(crate) fn twin_rows_kept(module: &Module) -> Vec<bool> {
    let n = module.class_layouts.len();
    let has_twin = |cl: &ClassLayoutMeta| cl.methods.iter().any(|m| m.twin_adapter_fid.is_some());
    if !module.class_layouts.iter().any(has_twin) {
        return vec![true; n];
    }
    let diag = std::env::var_os("TORAJS_TWIN_ROWS_DIAG").is_some();
    let escaped = crate::cmd_build_value_escape::escapes(
        module,
        &module.funcs,
        |t| match t {
            Type::Obj(s) | Type::Inline(s) => Some(s.0),
            _ => None,
        },
        // a class's release fn (`__drop_<C>`), by id: a user fn may
        // spell the same name
        |fid, name| module.class_drop_fns.contains(&fid) || keeps_instance(name),
    );
    if diag {
        for (s, why) in &escaped {
            eprintln!("[twin-rows] sid {s} escapes: {why}");
        }
    }
    let kept: Vec<bool> = (module.class_layouts.iter())
        .map(|cl| {
            has_twin(cl)
                && escaped.keys().any(|&s| {
                    (module.struct_layouts.get(s as usize))
                        .is_none_or(|fields| may_view(fields, cl))
                })
        })
        .collect();
    if !(module.class_layouts.iter().zip(&kept)).any(|(cl, k)| has_twin(cl) && !k) {
        return vec![true; n];
    }
    let why = match program_facts(module) {
        (Some(road), _) => Some(road),
        (None, true) if !escaped.is_empty() => Some("a class hierarchy with an escaping instance"),
        _ if !crate::cmd_build_elide_class::class_cells_stay_private(module) => {
            Some("a class cell leaves its prologue")
        }
        _ => None,
    };
    if let Some(why) = why {
        if diag {
            eprintln!("[twin-rows] keep all: {why}");
        }
        return vec![true; n];
    }
    kept
}

/// (the first call taking a road to a prototype, a user class
/// hierarchy). Each callee is classified once, on its first call.
fn program_facts(module: &Module) -> (Option<&str>, bool) {
    // 0 unseen, 1 neither, 2 a road, 3 the hierarchy link
    let mut kind = vec![0u8; module.funcs.len()];
    let mut hierarchy = false;
    for i in module
        .funcs
        .iter()
        .flat_map(|f| &f.blocks)
        .flat_map(|b| &b.insts)
    {
        let InstKind::Call(fid, _) = &i.kind else {
            continue;
        };
        let name = module.funcs[fid.0 as usize].name.as_str();
        let k = &mut kind[fid.0 as usize];
        if *k == 0 {
            *k = if KEEP_ALL_PREFIXES.iter().any(|p| name.starts_with(p)) {
                2
            } else if name == HIERARCHY {
                3
            } else {
                1
            };
        }
        match *k {
            2 => return (Some(name), hierarchy),
            3 => hierarchy = true,
            _ => {}
        }
    }
    (None, hierarchy)
}

/// Whether a value of a struct type with `fields` may be an instance
/// of the class `cl`: its fields are a prefix of the class's, in
/// offset order. A row without field metadata answers yes.
fn may_view(fields: &[(PropKey, Type)], cl: &ClassLayoutMeta) -> bool {
    if cl.field_metadata.is_empty() {
        return true;
    }
    let mut own: Vec<_> = cl.field_metadata.iter().collect();
    own.sort_by_key(|fm| fm.offset);
    fields.len() <= own.len()
        && fields
            .iter()
            .zip(&own)
            .all(|((name, _), fm)| fm.name == *name)
}
