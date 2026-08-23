//! ArrayBuffer / TypedArray / DataView substrate
//! (RFC 20260823-typedarray-substrate).
//!
//! The byte store is §25.1's ArrayBuffer; everything else in this
//! crate is a *view* onto one. Two states the layout names outright,
//! because the spec names them and a side flag would drift:
//!
//! - **detached** is `data == null` (§25.1.3.3 DetachArrayBuffer sets
//!   `[[ArrayBufferData]]` to null and there is nothing else to read);
//! - **not resizable** is `max_byte_len == -1`, i.e. the object has no
//!   `[[ArrayBufferMaxByteLength]]` at all. Absent is a real state and
//!   is not the same as a maximum of zero.
//!
//! A resizable buffer allocates its **maximum** up front and moves
//! only `byte_len` on `resize`. That keeps every live view's data
//! pointer valid across a resize, which is what makes §10.4.5's
//! "re-derive the length on every access" cheap enough to actually do
//! on every access.

pub mod arraybuffer;
pub mod arraybuffer_ops;
pub mod arraybuffer_print;
pub mod binary16;
pub mod typedarray;
pub mod typedarray_ctor;
pub mod typedarray_elem;
pub mod typedarray_from;
pub mod typedarray_inplace;
pub mod typedarray_print;
pub mod typedarray_props;
pub mod typedarray_search;
pub mod typedarray_span;
