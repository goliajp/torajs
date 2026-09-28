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
pub(super) const IDENTITY_PUSH_CALLS: &[&str] = &["__torajs_arr_push"];
/// Writes argument 1 into an element; returns nothing.
pub(super) const VOID_PUSH_CALLS: &[&str] = &["__torajs_arr_push_unchecked"];
/// Touches the header or frees the block; no element value effect.
/// The length-lock raisers belong here too: they read the flags and
/// either do nothing or record a TypeError, and the one that moves
/// elements first (`shift_guard`'s partial shift) only copies values
/// the slots already held.
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
    "__torajs_arr_shift_guard",
    "__torajs_arr_drop",
    "__torajs_arr_drop_scalar",
    "__torajs_arr_drop_any",
    "__torajs_arr_drop_heap",
    "__torajs_arr_drop_str_elems",
];
