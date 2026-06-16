// S129-4 — Array<Any>.concat(Array<typed T>) mixed-type wedge
// regression guard. Per ES §23.1.3.4 concat returns a new array
// of the receiver's spread + each arg's spread. Pre-fix tora
// bailed at check.rs with "expected Array(Any), got Array(Number)"
// — concat's typed sig required receiver and arg to share the
// elem type. For Array<Any> receiver this rejects every typed
// caller pattern (`xs.concat([99])` with `xs: any[]`).
//
// Three-tier fix mirror of S129-3 flat:
// check.rs Array<Any>.concat arg sig = Type::Any (typed Array<U>
// passes type-check); ssa-lower peeks the arg's SSA type and
// routes typed-arg to __torajs_arr_extend_typed_into_any with
// the U-derived elem tag (1=Bool/2=I64/3=F64/4=Heap). Same Array
// <Any> mixed-typed escape series as S128-1..3 push/fill/flat.

function getAny(): any[] { return [1, "x"]; }
const xs: any[] = getAny();

// I64 src — elem_tag=2 path
const a = xs.concat([99, 100]);
console.log(a.length);  // 4
console.log(a[0]);       // 1
console.log(a[1]);       // x
console.log(a[2]);       // 99
console.log(a[3]);       // 100

// String (heap) src — elem_tag=4 path
const b = xs.concat(["y", "z"]);
console.log(b.length);  // 4
console.log(b[2]);       // y
console.log(b[3]);       // z

// Bool src — elem_tag=1 path
const c = xs.concat([true, false]);
console.log(c.length);  // 4
console.log(c[2]);       // true
console.log(c[3]);       // false

// Array<Any>.concat(Array<Any>) regression — original same-type
// shape still routes through arr_concat memcpy.
const d = xs.concat(getAny());
console.log(d.length);  // 4
console.log(d[2]);       // 1
console.log(d[3]);       // x

// typed Array<T>.concat(Array<T>) regression — typed receiver
// path unchanged.
const ns: number[] = [1, 2];
const e = ns.concat([3, 4]);
console.log(e.length);  // 4
console.log(e[3]);       // 4
