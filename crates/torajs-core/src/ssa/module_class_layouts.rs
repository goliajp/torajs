//! Class-layout reflection metadata specs — split out of
//! `module_methods.rs` (file-size line, rotation 296): the
//! `ClassLayoutMeta` / `FieldMetaSpec` / `MethodMetaSpec` trio the
//! lowering populates and torajs-link bakes into the per-class
//! `.__class_fields_<i>` / `.__class_methods_<i>` rodata globals,
//! plus the coarse `Type` → 8-bit discriminator they share.

use super::FuncId;
use crate::ast::PropKey;

/// W-J Phase A3 (RFC 20260614-w-j-struct-reflect §3 A3) — per-field
/// metadata for the reflection consumers (Phase B `gOPD` struct cell
/// arm / Phase C `Object.keys`/`values`/`entries` / Phase D
/// `inspect.rs` Tag::Obj walker). Carried by `ClassLayoutMeta` and
/// later lowered by the link layer into a per-class
/// `.__class_fields_<i>` rodata array (Phase A3b — emit substrate +
/// chain-fixup wiring is a separate chunk).
///
/// `type_tag` is a coarse [`Type`] discriminator (Any / I64 / F64 /
/// Bool / Str / Substr / Arr / Obj / Closure / Map / Set / Date /
/// RegExp / Promise / Symbol / BigInt / Ptr / …). 8-bit fits the
/// current ~24 variants per RFC §6 R3; future refinement (Arr<I64>
/// vs Arr<Str> precision) can either widen to 16-bit or fall back
/// to a tag-walker second lookup.
#[derive(Debug, Clone)]
pub struct FieldMetaSpec {
    /// Field name as it appears in the struct literal / class decl.
    pub name: PropKey,
    /// Byte offset within the instance (`OBJ_HEADER_SIZE + i*8`).
    pub offset: u32,
    /// Coarse type discriminator; see [`field_type_tag_of`].
    pub type_tag: u8,
}

/// RFC 20260714-t262-top-clusters 刀 4 — one runtime-dispatchable
/// class method: the surface name plus the boxed dual-entry adapter
/// synthesized for its `__cm_<C>__<m>` body (uniform
/// `(this-as-env, argv, argc) -> AnyValue` ABI). Lowered into the
/// per-class `.__class_methods_<i>` rodata global so an any-held
/// class instance can resolve `c.next()` by name at runtime.
#[derive(Debug, Clone)]
pub struct MethodMetaSpec {
    /// Method name as declared (`next`, not `__cm_Gen__next`).
    pub name: String,
    /// The boxed adapter fn — resolves to a vaddr through the
    /// `__torajs_fn_<i>` sym convention (vtable slots' mechanism).
    pub adapter_fid: FuncId,
    /// S2.38 — the `__cm_` body never reads its receiver param
    /// (proven at the SSA level). Baked into the MethodMeta flags
    /// word so a bare / primitive-`this` call of the reified face
    /// runs the body with a null receiver instead of the
    /// this-undefined TypeError (ES §10.2.1.2 — a this-free body
    /// runs regardless of the thisArgument).
    pub this_free: bool,
    /// RFC 20260804-method-rebind-generic-body blade 3 — the
    /// receiver-polymorphic `__cmany_` twin's boxed adapter, when the
    /// method minted one (this-reading body, no super call). The
    /// reified face's receiver guard routes a foreign receiver here;
    /// `None` bakes a 0 twin_ptr (guard fail keeps the mono path —
    /// the recorded super-route residue).
    pub twin_adapter_fid: Option<FuncId>,
    /// 404-01 — `adapter_fid` IS the receiver-polymorphic twin
    /// (recv-first calling convention: the receiver box rides
    /// argv[0], the env argument is dropped). Minted for a GENERIC
    /// class's rows, whose mono bodies read fields at one
    /// specialization's offsets and would misread another's. Baked
    /// as MethodMeta flags bit 1; env-slot dispatch sites must not
    /// invoke such a record through the env channel.
    pub twin_primary: bool,
    /// 508-03 — this row is one the class DECLARES, not one it
    /// inherits. The table itself stays merged (it is what the typed
    /// `struct_method` dispatch resolves against, in one lookup), but
    /// only a declared row becomes an own property of `__proto_<C>`:
    /// the subclass prototype's [[Prototype]] already points at the
    /// parent's, so a copy there is both a phantom own name and a
    /// shadow that survives re-linking the chain. Baked as MethodMeta
    /// flags bit 2.
    pub declared_here: bool,
}

#[derive(Debug, Clone)]
pub struct ClassLayoutMeta {
    /// Class name (informational; useful for naming a per-class
    /// debug symbol, but the runtime indexes by tag).
    pub class_name: String,
    /// Byte offsets within an instance where refcounted heap-pointer
    /// fields live (already includes OBJ_HEADER_SIZE = 24). Used by
    /// the cycle collector's per-tag visitor to enumerate children
    /// during mark/scan/collect.
    pub child_offsets: Vec<u32>,
    /// W-J Phase A3 — per-field name + offset + type_tag for the
    /// reflection consumers. Populated in `ssa_lower` at every
    /// `ClassLayoutMeta::push` site (named class + anonymous
    /// ObjectLit). Empty Vec means no metadata available; Phase A3b
    /// will turn this into a `__torajs_class_layouts` entry's
    /// `field_metadata_ptr` slot via per-class rodata emit.
    pub field_metadata: Vec<FieldMetaSpec>,
    /// L3b #4 — `true` for declared classes, `false` for anonymous
    /// struct shapes. Lowered into the outer entry's flags word
    /// (bit 0) so the runtime Obj drop can mirror the typed path's
    /// cycle-root policy: only named-class instances register in the
    /// cycle buffer (the lower-emitted anon-struct drop skips the
    /// buffer scrub for speed, so a runtime-buffered anon struct
    /// would leave a dangling buffer entry behind).
    pub is_named: bool,
    /// 405-04 knife 2 fix (rotation 408) — `true` for a GENERIC
    /// specialization row (a mono factory's per-factory tag, 404-01):
    /// the row wears the class's identity but its proto/class
    /// registry slots are never filled, so the runtime registry
    /// aliases it to the main tag by name. Baked as outer-entry
    /// flags bit 1; the alias resolver triggers ONLY on rows carrying
    /// this bit (a non-generic tag with an empty slot keeps the null
    /// answer — several consumers' termination logic depends on it).
    pub is_generic: bool,
    /// 刀 4 — runtime-dispatchable methods (inherited included, the
    /// vtable walk's resolution). Empty for anonymous shapes and for
    /// classes whose methods all failed adapter synthesis.
    pub methods: Vec<MethodMetaSpec>,
}

/// W-J Phase A3 — coarse `Type` → 8-bit `type_tag` for [`FieldMetaSpec`].
/// Discriminator values are kept stable across builds so the link layer
/// + runtime helper can decode in lockstep. Unknown variants fold to
/// `0` (Any) — reflection sees them as opaque NaN-box cells and the
/// downstream consumer can do its own second-lookup via the heap
/// header's `type_tag` if it needs precision.
pub fn field_type_tag_of(ty: super::Type) -> u8 {
    use super::Type;
    match ty {
        Type::Any => 0,
        Type::I32 | Type::I64 => 1,
        Type::F64 => 2,
        Type::Bool => 3,
        Type::Str => 4,
        Type::Substr => 5,
        Type::Arr(_) => 6,
        Type::Obj(_) => 7,
        Type::Closure(_) => 8,
        Type::Map => 9,
        Type::Set => 10,
        Type::Date => 11,
        Type::RegExp => 12,
        Type::Promise => 13,
        Type::Symbol => 14,
        Type::BigInt => 15,
        Type::WeakRef => 16,
        Type::WeakMap => 17,
        Type::WeakSet => 18,
        Type::MapIter => 19,
        Type::ArrIter => 20,
        Type::Ptr => 21,
        _ => 0,
    }
}
