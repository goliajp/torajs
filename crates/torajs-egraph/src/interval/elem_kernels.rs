//! The array header offsets and the kernel name lists the element
//! analysis reads: which runtime calls define, extend, write or leave
//! an array's elements. Split from `elem.rs` at the file-size line.
//! A call on no list takes the array out of the analysis — new
//! kernels join a list only once their element effect is read.

/// Array header offsets. Mirrors `torajs_core::ssa_lower`'s
/// `ARR_LEN_OFF` / `ARR_PROPS_OFF` / `ARR_DATA_PTR_OFF` (private to
/// that crate) and, through them, `torajs_arr::layout`.
/// The heap header: refcount word and flags half-word. The two
/// spellings lowering reads them with — `Load(I64, arr, 0)` then
/// `LShr 48`, and `Load(I32, arr, 4)` then a `<< 16` mask — are the
/// [[Extensible]] / [[Writable]] guards on push / pop / shift / fill,
/// and a read of either says nothing about the elements.
pub(super) const ARR_HEADER_WORD_OFF: u64 = 0;
pub(super) const ARR_HEADER_FLAGS_OFF: u64 = 4;
pub(super) const ARR_LEN_OFF: u64 = 8;
pub(super) const ARR_CAP_HEAD_OFF: u64 = 16;
pub(super) const ARR_DATA_PTR_OFF: u64 = 32;

/// Defines a fresh array whose elements are all still unwritten.
pub(super) const ALLOC_CALLS: &[&str] = &["__torajs_arr_alloc", "__torajs_arr_alloc_pooled"];
/// Returns the same array (possibly reallocated, existing elements
/// carried over) and writes no element.
pub(super) const IDENTITY_CALLS: &[&str] = &["__torajs_arr_reserve"];
/// Returns the same array AND writes argument 1 into an element.
pub(super) const IDENTITY_PUSH_CALLS: &[&str] = &["__torajs_arr_push", "__torajs_arr_push_cold"];
/// Writes argument 1 into an element; returns nothing.
pub(super) const VOID_PUSH_CALLS: &[&str] = &["__torajs_arr_push_unchecked"];
/// Kernels that may leave an element holding a value no write spells:
/// the shift lock raiser moves the elements down and, under a
/// non-writable length, deletes the last index (it reads `undefined`)
/// before it refuses the length write.
pub(super) const HOLE_WRITE_CALLS: &[&str] = &["__torajs_arr_shift_guard"];
/// Admitted kernels that leave the length field as they found it:
/// `reserve` grows capacity only, the species guard reads the props
/// slot, `mark_kind` reads the length to walk the elements. Every
/// other admitted call is an opaque length def for the length cells.
pub(super) const LEN_INERT_CALLS: &[&str] = &[
    "__torajs_arr_reserve",
    "__torajs_arr_species_guard",
    "__torajs_arr_mark_kind",
];
/// Touches the header or frees the block; no element value effect.
/// The shift kernel moves the head past slot 0 and drops the length
/// (and moves the hole marks a tracked array never carries): the
/// slots still in bounds keep values some write spelled.
/// The tail-delete lock raiser belongs here too: it reads the flags and
/// either does nothing or records a TypeError.
/// The species guard (`Array.prototype.map` and its family, §9.4.2.3)
/// reads the props slot and answers on a null bag; a bag — the only
/// way to a `constructor` getter that could touch the array — is
/// grown by kernels this list already leaves out, so on a tracked
/// array the guard reads and nothing more. Off the list it took the
/// source of every typed `map` out of the analysis, and the map loop
/// re-read the source's data pointer and capacity per step.
pub(super) const VOID_INERT_CALLS: &[&str] = &[
    "__torajs_arr_species_guard",
    "__torajs_arr_mark_kind",
    "__torajs_arr_delete_tail_guard",
    "__torajs_arr_drop",
    "__torajs_arr_drop_scalar",
    "__torajs_arr_drop_any",
    "__torajs_arr_drop_heap",
    "__torajs_arr_drop_str_elems",
    "__torajs_arr_drop_leaf_objs",
    "__torajs_arr_drop_leaf_objs_by_cell",
    "__torajs_arr_drop_leaf_instances",
    "__torajs_arr_drop_leaf_instances_by_cell",
    "__torajs_arr_shift",
];
