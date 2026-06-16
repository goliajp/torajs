// S132 — `<typed refcounted> as any` SSA-lower box bridge. TS `as any`
// is a compile-time type erase; tora's materialize tier must emit a
// real `box_to_any` so downstream Any-arm dispatchers (Object.values /
// keys / entries / gOPD) see Type::Any and route through the W-J
// walker instead of the homogeneous-struct fast path.
//
// Pre-S132: `lower_as_cast` only boxed primitive (I64/I32/F64/Bool)
// and Ptr (undefined/null) inner types; refcounted types
// (Obj/Arr/Str/Map/...) fell through with the typed operand intact.
// Object.values then dispatched through the typed Obj arm, which
// assumes a homogeneous struct (elem_ty = layout[0].1) and emitted
// fixed-type Loads for every field — a mixed-type struct
// (`class C { n: number; s: string }`) silently returned the str
// pointer as a Number. S132 closes that gap by routing all
// refcounted inner types through `box_to_any_from_expr` too.

// Mixed-type fields (declaration order: number / string / boolean /
// number). Walker must per-field decode through field_metadata's
// type_tag, not assume layout[0].
class Mixed {
  n: number
  s: string
  b: boolean
  k: number
  constructor(n: number, s: string, b: boolean, k: number) {
    this.n = n
    this.s = s
    this.b = b
    this.k = k
  }
}
const mx = new Mixed(7, "world", true, 42)

// Pre-fix: `vs[1]` decoded the str ptr through layout[0].1=I64 and
// printed the raw VA. Post-fix: walker reads field[1].type_tag=Str
// and unboxes a real Str cell.
const vs = Object.values(mx as any)
console.log("vs.len", vs.length)
console.log("vs.0", vs[0], "typeof", typeof vs[0])
console.log("vs.1", vs[1], "typeof", typeof vs[1])
console.log("vs.2", vs[2], "typeof", typeof vs[2])
console.log("vs.3", vs[3], "typeof", typeof vs[3])

// Reversed field order — string first, number second. Confirms the
// type erase is per-cast, not anchored to a particular field 0 type.
class Reversed {
  s: string
  n: number
  constructor(s: string, n: number) {
    this.s = s
    this.n = n
  }
}
const rv = new Reversed("alpha", 9)
const rvs = Object.values(rv as any)
console.log("rvs.0", rvs[0], "typeof", typeof rvs[0])
console.log("rvs.1", rvs[1], "typeof", typeof rvs[1])

// Object.entries via `as any` — same cast bridge, same walker. Each
// inner pair is `[name, boxed_value]` (Arr<Arr<Any>>).
const en = Object.entries(mx as any)
console.log("en.len", en.length)
console.log("en[0] key", en[0][0])
console.log("en[1] key", en[1][0])
console.log("en[0] val", en[0][1], "typeof", typeof en[0][1])
console.log("en[1] val", en[1][1], "typeof", typeof en[1][1])

// Object.keys via `as any` — names only, no value cell decode, but
// shares the same Type::Any operand-typing prerequisite. Index reads
// to avoid the Arr<Str> multi-arg console.log coercion gap.
const ks = Object.keys(mx as any)
console.log("ks.len", ks.length)
console.log("ks.0", ks[0])
console.log("ks.1", ks[1])
console.log("ks.2", ks[2])
console.log("ks.3", ks[3])

// gOPD via `as any` — already correct before S132 (the gOPD arm
// already handled struct cells via the Any-typed param of the
// helper). Regression guard that the new box path doesn't break it.
const ds = Object.getOwnPropertyDescriptor(mx as any, "s")
console.log("ds.value", ds!.value)
