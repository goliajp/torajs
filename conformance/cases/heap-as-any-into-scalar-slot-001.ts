// A heap value reaching a scalar annotation through `as any` used to
// have its POINTER read as the scalar. `let s: string = ([4] as any)`
// answered "\u0001" — one control character, the low byte of an
// address — and the number and boolean slots read the same bits as
// their own widths.
//
// The cause is that `as any` lowers to the value's own representation
// while the checker admits Any into the slot, so the operand's static
// type is the heap one and the slot's Any conversions never saw it.
// The argument position has boxed for this since it was written
// (`heap_into_primitive`); the let annotation did not.
//
// What it answers now is this project's existing stance on a lying
// annotation: the spec's conversion for the slot's type, the same one
// `let n: number = (t)` has always given. bun keeps the value instead,
// because TS annotations vanish at runtime — that difference is the
// registered erasure gap and is NOT what this file is about. It is
// about the answer being a defined value rather than an address.
// (With the checked-cast gate on, every line here throws a TypeError
// instead, which is the other way to stop reading a pointer as a
// number.)
//
// Registered separately, found while writing this: a value whose
// static type is ALREADY Any never reaches the boxing step, and
// `const v: any = new Map(); let m: number = v` answers 0 rather than
// NaN. Same for a RegExp. That is the Any arm's own ToNumber, not
// this path.

let s: string = [4] as any;
console.log("arr->str", JSON.stringify(s));

let n: number = [4] as any;
console.log("arr->num", n);

let b: boolean = [] as any;
console.log("arr->bool", b);

let o: string = { a: 1 } as any;
console.log("obj->str", JSON.stringify(o));

// A string through the same path is unchanged — it was never the
// broken case.
let t: string = "ok" as any;
console.log("str->str", t);
