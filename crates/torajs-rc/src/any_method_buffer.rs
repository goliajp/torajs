//! The buffer families' method ids — `ArrayBuffer`, `%TypedArray%`
//! and `DataView` (RFC 20260823-typedarray-substrate), plus
//! `Uint8Array`'s four text conversions.
//!
//! Split out of [`super::any_method`] when that file reached the
//! 500-line limit; the seam is the one
//! [`super::any_method_intern::buffer_method_id`] and
//! [`super::any_method_meta`]'s `buffer_method_meta` already draw.
//!
//! **The id space is shared with `any_method` and `any_method_iter`,
//! and it is append-only** — ids are baked into call sites, so a
//! reorder silently re-keys every one of them. `mid_ids_are_unique`
//! checks all three files together.

/// §25.1.6.6 `ArrayBuffer.prototype.resize`
/// (RFC 20260823-typedarray-substrate 刀 1). `slice` reuses
/// [`ANY_METHOD_SLICE`] — same name, and the receiver tag is what
/// picks the body. Same append-only ABI contract as the ids above.
pub const ANY_METHOD_RESIZE: i64 = 178;

/// §23.2.3.28 `%TypedArray%.prototype.subarray`
/// (RFC 20260823-typedarray-substrate 刀 5). The only slab-A name
/// that is not already shared with `Array.prototype`, so it is the
/// only one that needed an id. Same append-only ABI contract.
pub const ANY_METHOD_SUBARRAY: i64 = 179;

/// §25.1.6.7 `ArrayBuffer.prototype.transfer` / §25.1.6.8
/// `transferToFixedLength` (RFC 20260823-typedarray-substrate 刀 8).
/// The only way a program detaches a buffer without a host hook, and
/// therefore what test262's `$DETACHBUFFER` runs on. Two ids rather
/// than one flag: they are two function objects with two names, and
/// the reflection faces read the name out of this table. Same
/// append-only ABI contract as the ids above.
pub const ANY_METHOD_TRANSFER: i64 = 180;
pub const ANY_METHOD_TRANSFER_TO_FIXED_LENGTH: i64 = 181;

/// §25.3.4 `DataView.prototype.get*` / `set*` (RFC
/// 20260823-typedarray-substrate 刀 7) — one mid per method name;
/// the dispatcher folds each pair onto the two shared kernels.
pub const ANY_METHOD_DV_GET_INT8: i64 = 182; // `getInt8`
pub const ANY_METHOD_DV_SET_INT8: i64 = 183; // `setInt8`
pub const ANY_METHOD_DV_GET_UINT8: i64 = 184; // `getUint8`
pub const ANY_METHOD_DV_SET_UINT8: i64 = 185; // `setUint8`
pub const ANY_METHOD_DV_GET_INT16: i64 = 186; // `getInt16`
pub const ANY_METHOD_DV_SET_INT16: i64 = 187; // `setInt16`
pub const ANY_METHOD_DV_GET_UINT16: i64 = 188; // `getUint16`
pub const ANY_METHOD_DV_SET_UINT16: i64 = 189; // `setUint16`
pub const ANY_METHOD_DV_GET_INT32: i64 = 190; // `getInt32`
pub const ANY_METHOD_DV_SET_INT32: i64 = 191; // `setInt32`
pub const ANY_METHOD_DV_GET_UINT32: i64 = 192; // `getUint32`
pub const ANY_METHOD_DV_SET_UINT32: i64 = 193; // `setUint32`
pub const ANY_METHOD_DV_GET_FLOAT16: i64 = 194; // `getFloat16`
pub const ANY_METHOD_DV_SET_FLOAT16: i64 = 195; // `setFloat16`
pub const ANY_METHOD_DV_GET_FLOAT32: i64 = 196; // `getFloat32`
pub const ANY_METHOD_DV_SET_FLOAT32: i64 = 197; // `setFloat32`
pub const ANY_METHOD_DV_GET_FLOAT64: i64 = 198; // `getFloat64`
pub const ANY_METHOD_DV_SET_FLOAT64: i64 = 199; // `setFloat64`
pub const ANY_METHOD_DV_GET_BIGINT64: i64 = 200; // `getBigInt64`
pub const ANY_METHOD_DV_SET_BIGINT64: i64 = 201; // `setBigInt64`
pub const ANY_METHOD_DV_GET_BIGUINT64: i64 = 202; // `getBigUint64`
pub const ANY_METHOD_DV_SET_BIGUINT64: i64 = 203; // `setBigUint64`
/// §25.1.6.13 `get ArrayBuffer.prototype.resizable` — reified
/// accessor getter (never interned by name; reachable only via the
/// carried-mid re-dispatch, the GET_SIZE posture).
pub const ANY_METHOD_GET_RESIZABLE: i64 = 204;

/// §23.2.3's four `Uint8Array.prototype` text conversions. They are
/// `Uint8Array`'s alone, not `%TypedArray%.prototype`'s — the brand
/// check that says so lives at the call, because the heap tag is
/// `TypedArray` for all eleven element types and the id space is not
/// where that distinction can be drawn.
///
/// 207 continues the tail of BOTH id files (see the note above:
/// `any_method_iter` grows the same space and reached 205).
pub const ANY_METHOD_TO_BASE64: i64 = 207;
pub const ANY_METHOD_TO_HEX: i64 = 208;
pub const ANY_METHOD_SET_FROM_BASE64: i64 = 209;
pub const ANY_METHOD_SET_FROM_HEX: i64 = 210;
