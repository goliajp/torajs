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
// What it answers now: the binding holds the value itself, as the
// spec binds it — an `as any` cast converts nothing and the annotation
// has no runtime meaning, so a scalar slot initialized with one holds
// the box from its declaration, and so does one initialized with a
// read of an `any` value. bun answers the same.

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
