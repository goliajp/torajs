//! Runtime constants the SSA spells inline.

/// Mach-O names of the two runtime statics user code reads INLINE
/// (`GlobalRef` + `Load`) instead of calling an accessor — rotation
/// 470, each such call was a measurable slice of a method-call loop:
/// the in-flight-throw flag `torajs_throw::__torajs_throw_active`
/// (read after every call that may raise; the e-graph's
/// self-tail-call matcher anchors on it) and the live-WeakRef-
/// observer count `torajs_rc::__torajs_weakref_active` (read before
/// a class instance dies).
pub const THROW_ACTIVE_SYM: &str = "___torajs_throw_active";
pub const WEAKREF_ACTIVE_SYM: &str = "___torajs_weakref_active";
/// The any-slot tag of a heap cell as `__torajs_anyv_box_from_pair`
/// reads it (torajs-rc `AnySlotTag::Heap`); every other tag boxes an
/// immediate, which no rc kernel touches.
pub const ANY_SLOT_TAG_HEAP: i64 = torajs_rc::AnySlotTag::Heap as i64;
/// Heap-header flag bits (the u16 at +6) that change without a call
/// naming the object: the cycle collector's buffered and color bits.
/// Every other flag moves only inside a runtime call handed the object.
pub const HEADER_FLAGS_COLLECTOR: u16 = torajs_rc::FLAG_BUFFERED | torajs_rc::COLOR_MASK;
/// The header word of a dense `any[]`: `FLAG_ARR_ANY` in the flag
/// half, as the inline element read tests it (`and word, mask` then
/// `icmp eq` against this).
pub const HEADER_WORD_ARR_ANY: i64 = (torajs_rc::FLAG_ARR_ANY as i64) << 48;
