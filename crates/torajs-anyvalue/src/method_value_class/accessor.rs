//! Class-accessor faces (RFC 20260718-accessor-reify 刀 2) — split
//! from the parent when the K2b unit-face work pushed it past the
//! 500-line hard limit. The parent answers "how a class METHOD lands
//! on the prototype as a callable face"; this answers the same for a
//! GETTER/SETTER half, whose cell carries two more slots (the §17
//! `.name` and the ES `length`) and therefore its own sentinel,
//! receiver-guard offsets and invoke entry. Body verbatim; a child
//! module reaches the parent's private consts and externs directly.

use core::ffi::c_void;

use torajs_rc::{FLAG_STATIC_LITERAL, Tag};

use super::*;
use crate::nanbox::VALUE_UNDEFINED;

/// Extended cell layout for accessor faces — two extra slots after
/// the adapter carry the reflection metadata a bare method cell
/// lacks: the interned `.name` Str ("get x" / "set x", §17 prefix
/// form) and the ES `length`.
const ACC_CELL_NAME_OFF: usize = 56;
const ACC_CELL_LEN_OFF: usize = 64;
/// 629-02 — the accessor face's receiver-guard pair, the twin of a
/// METHOD face's `(class_tag, twin)` at 56/64: the owning class's tag
/// (0 = every receiver rides the twin — a twin-primary row) and the
/// `__cmany_` twin's boxed adapter (0 = no twin; a this-free body).
const ACC_CELL_TAG_OFF: usize = 72;
const ACC_CELL_TWIN_OFF: usize = 80;
/// The evaluation's class object a UNIT face holds (RFC
/// 20260917-class-eval-unit K2b, the accessor half): 0 on every
/// immortal one-per-class face, whose block ends before this word.
const ACC_CELL_UNIT_OFF: usize = 88;
const ACC_CELL_SIZE: usize = 88;
const ACC_UNIT_CELL_SIZE: usize = 96;

/// Boxed dual entry of a reified class-accessor face — its own
/// sentinel (distinct from [`class_bare_entry`]) so the reflection
/// probes know the extended slots exist; a bare call is the same
/// `this = undefined` TypeError.
pub(super) unsafe extern "C" fn class_accessor_bare_entry(
    _env: *mut c_void,
    _argv: *const u64,
    _argc: i64,
) -> u64 {
    unsafe {
        __torajs_throw_type_error(
            c"class accessor called without a receiver (this is undefined)".as_ptr(),
        );
    }
    VALUE_UNDEFINED
}

/// Mint one immortal class-accessor face carrying `adapter` plus its
/// reflection metadata. `name` transfers — a fresh Str cell the face
/// keeps forever (the face itself never drops). `(class_tag, twin)`
/// is the receiver guard [`__torajs_class_accessor_face_invoke`]
/// reads (629-02): the mono adapter reads the receiver at the owning
/// class's offsets and is sound only for that class's instances;
/// any other receiver — a dynobj through `Reflect.get(C.prototype,
/// k, o)`, a subclass, a struct of another class — takes the twin.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_accessor_cell_new(
    adapter: u64,
    name: *mut u8,
    length: u64,
    class_tag: u64,
    twin: u64,
) -> *mut u8 {
    // SAFETY: a spec whose `unit` is 0 — the immortal shape.
    unsafe {
        mint(AccessorCellSpec {
            adapter,
            name,
            length,
            class_tag,
            twin,
            unit: 0,
        })
    }
}

/// K2b, the accessor half — one evaluation's accessor face of a unit
/// class. §15.7.14 gives every evaluation its own function objects,
/// so the face is minted per evaluation and is MORTAL: no
/// `FLAG_STATIC_LITERAL`, a real [`accessor_face_drop`] that also
/// releases the `.name` Str, and an [`accessor_face_trace`] over the
/// held class object (class → prototype → pair → face → class is a
/// cycle by construction). Before this every evaluation of a unit
/// class with a private accessor leaked two faces and two name Strs.
///
/// # Safety
/// `name` is an owned Str cell; `unit` is a live heap cell pointer
/// (never 0 — use the immortal mint for the shared prototype).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_accessor_cell_new_unit(
    adapter: u64,
    name: *mut u8,
    length: u64,
    class_tag: u64,
    twin: u64,
    unit: u64,
) -> *mut u8 {
    // SAFETY: caller contract above.
    unsafe {
        mint(AccessorCellSpec {
            adapter,
            name,
            length,
            class_tag,
            twin,
            unit,
        })
    }
}

/// What one accessor face is made of — a struct for the same reason
/// the method face has one: `adapter`, `twin` and `unit` are all bare
/// words and swapping two of them compiles.
struct AccessorCellSpec {
    adapter: u64,
    name: *mut u8,
    length: u64,
    class_tag: u64,
    twin: u64,
    unit: u64,
}

/// Mint one reified class-accessor face: immortal when `spec.unit` is
/// 0, mortal (one per evaluation, one word longer) when it is not.
///
/// # Safety
/// `spec.name` transfers; `spec.unit` is 0 or a live heap cell this
/// face takes a `+1` on.
unsafe fn mint(spec: AccessorCellSpec) -> *mut u8 {
    let size = if spec.unit != 0 {
        ACC_UNIT_CELL_SIZE
    } else {
        ACC_CELL_SIZE
    };
    // SAFETY: fresh allocation of `size`, fully initialized below.
    unsafe {
        let layout = core::alloc::Layout::from_size_align(size, 8).unwrap();
        let cell = std::alloc::alloc_zeroed(layout);
        *(cell as *mut u32) = 1;
        *(cell.add(4) as *mut u16) = Tag::Closure as u16;
        *(cell.add(6) as *mut u16) = if spec.unit != 0 {
            0
        } else {
            FLAG_STATIC_LITERAL
        };
        *(cell.add(CLOSURE_FN_ADDR_OFF) as *mut u64) = class_native_entry as *const () as u64;
        *(cell.add(CLOSURE_DROP_FN_OFF) as *mut u64) = if spec.unit != 0 {
            accessor_face_drop as *const () as u64
        } else {
            0
        };
        *(cell.add(CLOSURE_PROPS_OFF) as *mut u64) = 0;
        *(cell.add(CLOSURE_BOXED_ENTRY_OFF) as *mut u64) =
            class_accessor_bare_entry as *const () as u64;
        if spec.unit != 0 {
            *(cell.add(CLOSURE_TRACE_FN_OFF) as *mut u64) = accessor_face_trace as *const () as u64;
            torajs_rc::__torajs_rc_inc(spec.unit as *mut c_void);
            *(cell.add(ACC_CELL_UNIT_OFF) as *mut u64) = spec.unit;
        }
        *(cell.add(CLOSURE_CAP_BASE_OFF) as *mut u64) = spec.adapter;
        *(cell.add(ACC_CELL_NAME_OFF) as *mut u64) = spec.name as u64;
        *(cell.add(ACC_CELL_LEN_OFF) as *mut u64) = spec.length;
        *(cell.add(ACC_CELL_TAG_OFF) as *mut u64) = spec.class_tag;
        *(cell.add(ACC_CELL_TWIN_OFF) as *mut u64) = spec.twin;
        cell
    }
}

/// drop_fn of a MORTAL unit accessor face — release the expando bag,
/// the `.name` Str and the held class object, then the block (the
/// method face's `method_face_drop` shape).
///
/// # Safety
/// `env` is a live unit accessor-face cell at refcount 0.
unsafe extern "C" fn accessor_face_drop(env: *mut c_void) {
    unsafe {
        __torajs_cycle_unbuffer(env);
        let cell = env.cast::<u8>();
        let props = *(cell.add(CLOSURE_PROPS_OFF) as *const u64);
        if props != 0 {
            __torajs_value_drop_heap(props as *mut c_void);
        }
        let name = *(cell.add(ACC_CELL_NAME_OFF) as *const u64);
        if name != 0 {
            __torajs_value_drop_heap(name as *mut c_void);
        }
        let unit = *(cell.add(ACC_CELL_UNIT_OFF) as *const u64);
        if unit != 0 {
            __torajs_value_drop_heap(unit as *mut c_void);
        }
        std::alloc::dealloc(
            cell,
            core::alloc::Layout::from_size_align(ACC_UNIT_CELL_SIZE, 8).unwrap(),
        );
    }
}

/// trace_fn of a MORTAL unit accessor face — the held class object is
/// the cell's one traced child (the name Str holds nothing).
unsafe extern "C" fn accessor_face_trace(
    env: *mut c_void,
    visit: unsafe extern "C" fn(i64, *mut c_void, *mut c_void, *mut c_void),
    ctx: *mut c_void,
) {
    unsafe {
        let slot = env.cast::<u8>().add(ACC_CELL_UNIT_OFF);
        visit(
            0,
            *(slot as *const u64) as *mut c_void,
            slot as *mut c_void,
            ctx,
        );
    }
}

/// Invoke a reified class-accessor face against a receiver (629-02)
/// — the accessor-pair kernels' entry for a getter (empty argv) or a
/// setter (`argv[0]` = the value). The face's own guard decides the
/// shape: a receiver that IS an instance of the owning class takes
/// the mono adapter with the receiver in the env slot; any other
/// receiver takes the `__cmany_` twin recv-first, whose body reads
/// `this` through GetV — `Reflect.set(B.prototype, "s", 4, o)` runs
/// B's setter with the dynobj `o` as `this` instead of misreading the
/// dynobj's header as B's fields (the blade-3 guard the method faces
/// already have). A face with no twin (a this-free body) keeps the
/// mono path under every receiver; a non-cell receiver is the
/// bare-receiver TypeError.
///
/// # Safety
/// `face` is a live reified class-accessor face cell; `recv` / `argv`
/// carry valid AnyValue bit patterns; `argv` has `argc` readable
/// slots (null iff `argc == 0`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __torajs_class_accessor_face_invoke(
    face: *const c_void,
    recv: u64,
    argv: *const u64,
    argc: i64,
) -> u64 {
    unsafe {
        let Some(adapter) = class_method_adapter(face as *mut c_void) else {
            return VALUE_UNDEFINED;
        };
        if !crate::nanbox::is_cell(recv) {
            return class_bare_entry(core::ptr::null_mut(), argv, argc);
        }
        let entry = *(face.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        let (tag, twin) = if entry == class_accessor_bare_entry as *const () as u64 {
            (
                *(face.cast::<u8>().add(ACC_CELL_TAG_OFF) as *const u64),
                *(face.cast::<u8>().add(ACC_CELL_TWIN_OFF) as *const u64),
            )
        } else {
            (0, 0)
        };
        let recv_ptr = crate::nanbox::as_void_ptr(recv);
        let own_instance = tag != 0
            && (recv_ptr.cast::<u8>().add(4) as *const u16).read() == Tag::Obj as u16
            && u64::from((recv_ptr.cast::<u8>().add(8) as *const u32).read()) == tag;
        if twin != 0 && !own_instance {
            return crate::method_call_closure_dispatch::invoke_boxed_recv_first(
                face as *mut c_void,
                twin,
                recv,
                argv,
                argc,
            );
        }
        crate::method_call::invoke_boxed(recv_ptr, adapter, argv, argc)
    }
}

/// The `(name Str cell, length)` metadata when `ptr` is a reified
/// class-accessor face; `None` otherwise. The name cell is the
/// face's own immortal stake — callers rc_inc before handing out.
///
/// # Safety
/// `ptr` points at a live `Tag::Closure` heap cell.
pub(crate) unsafe fn class_accessor_meta(ptr: *mut c_void) -> Option<(*mut u8, u64)> {
    unsafe {
        let entry = *(ptr.cast::<u8>().add(CLOSURE_BOXED_ENTRY_OFF) as *const u64);
        if entry == class_accessor_bare_entry as *const () as u64 {
            Some((
                *(ptr.cast::<u8>().add(ACC_CELL_NAME_OFF) as *const u64) as *mut u8,
                *(ptr.cast::<u8>().add(ACC_CELL_LEN_OFF) as *const u64),
            ))
        } else {
            None
        }
    }
}
