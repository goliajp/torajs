//! Layout / tag mirror constants for the enumeration choosers —
//! split out of [`crate::obj_own_keys`] (rotation 354: the promise
//! arm pushed the parent over the 500-line file cap; constants moved
//! verbatim, the parent re-exports the whole face).

/// `HeapHeader::type_tag` mirror of `torajs_rc::Tag::DynObj` (locked
/// there); header field lives at byte offset 4. Shared with the
/// values/entries chooser twin (`obj_own_values.rs`).
pub(crate) const TAG_DYNOBJ: u16 = 14;
pub(crate) const HDR_TYPE_TAG_OFF: usize = 4;

/// `torajs_rc::Tag` mirrors for the ToObject dispatch arms
/// (chunk B1 — for-in RFC): Str / Arr / Closure / Obj cells each get
/// their own own-keys shape instead of the former non-struct throw.
pub(crate) const TAG_STR_CELL: u16 = 0;
pub(crate) const TAG_OBJ_CELL: u16 = 1;
pub(crate) const TAG_ARR_CELL: u16 = 2;
pub(crate) const TAG_CLOSURE_CELL: u16 = 3;
/// `Tag::Promise` mirror — the enumeration arm walks the +32 expando
/// bag (rotation 354).
pub(crate) const TAG_PROMISE_CELL: u16 = 8;

/// Promise-cell lazy expando slot (`torajs_dynobj::layout::
/// PROMISE_PROPS_OFF` mirror — +24 is the callback list).
pub(crate) const PROMISE_PROPS_OFF: usize = 32;
/// RFC 20260716 刀 5 (rotation 121 chunk 5) — primitive-wrapper cell
/// tags (`torajs_rc::Tag::{NumberWrapper,StringWrapper,BooleanWrapper}`
/// mirrors). Every wrapper carries a lazy expando dynobj at
/// `WRAPPER_PROPS_OFF` (mirror of the closure `+24` slot).
pub(crate) const TAG_NUMBER_WRAPPER: u16 = 21;
pub(crate) const TAG_STRING_WRAPPER: u16 = 22;
pub(crate) const TAG_BOOLEAN_WRAPPER: u16 = 23;
pub(crate) const WRAPPER_PROPS_OFF: usize = 16;
/// Wrapper `[[StringData]]` inner Str-cell ptr slot
/// (`torajs_rc::Tag::StringWrapper` layout: `[header:8][str_cell:8]`).
pub(crate) const WRAPPER_INNER_OFF: usize = 8;

/// torajs-arr layout mirrors — `len` u64 at +8, inline props-dynobj
/// slot at +24 (`torajs_arr::layout::ARR_PROPS_OFF`).
pub(crate) const ARR_LEN_OFF: usize = 8;
pub(crate) const ARR_PROPS_OFF: usize = 24;
/// Element storage pointer (`torajs_arr::layout::ARR_DATA_PTR_OFF`).
pub(crate) const ARR_DATA_PTR_OFF: usize = 32;
/// Closure env-cell props-dynobj slot (T-27 Function-as-Object,
/// mirror `torajs_anyvalue::member_get::CLOSURE_PROPS_OFF`).
pub(crate) const CLOSURE_PROPS_OFF: usize = 24;
/// Str payload length u32 at +8 (`torajs-str` layout).
pub(crate) const STR_LEN_OFF: usize = 8;

/// ShortStr NaN-box marker (`top16 == 0x0001`) + len bits 47..40
/// (mirror `torajs_anyvalue::nanbox` SSO layout).
pub(crate) const SHORT_STR_TOP16: u64 = 0x0001;

/// `torajs_dynobj::layout::BUCKET_FLAG_ENUMERABLE` mirror (bit 1).
pub(crate) const FLAG_ENUMERABLE: u64 = 1 << 1;

/// `AnySlotTag` heap tag (mirror torajs-anyvalue) — the tag
/// `unbox_tag` reports for any heap cell, including an AccessorPair.
pub(crate) const ANY_HEAP_TAG: i64 = 4;
/// `torajs_dynobj::accessor::TAG_ACCESSOR_PAIR` mirror.
pub(crate) const TAG_ACCESSOR_PAIR: u16 = 18;
/// Elem-kind chain stamping the entries outer array (`Arr<Arr<Any>>`:
/// heap elem = 4, inner FLAG_ARR_ANY blocks self-describe) so
/// kind-aware borrow readers (Object.fromEntries) decode the slots.
pub(crate) const KIND_CHAIN_HEAP: u64 = 4;
