// W-J Phase C2 — Object.values on an `any`-typed value that carries a
// static-layout struct cell. tora's compile-time `Object.values` arm
// requires a homogeneous-typed struct (so the element type can be
// inferred); an `any` binding hides the struct, so check.rs types the
// result as `Array<Any>` and SSA-lower routes to the struct_enum
// runtime walker. The walker reads each field slot and boxes it per
// its metadata type_tag (the same per-field decode the gOPD value slot
// uses), so a heterogeneous struct (number / string / boolean) round-
// trips correctly — something the typed compile-time arm rejects.
//
// Indexed reads keep the comparison on individual scalar values (the
// Array<Any> result has no `.join` member yet — that is a separate
// Array<Any> method-dispatch gap, not part of this struct arm).
class Point {
  x: number;
  y: number;
  label: string;
  active: boolean;
  constructor(x: number, y: number, label: string, active: boolean) {
    this.x = x;
    this.y = y;
    this.label = label;
    this.active = active;
  }
}
const p: any = new Point(3, 7, "origin", true);
const vs = Object.values(p);

// Value count is observable through the array length.
console.log(vs.length);

// Heterogeneous field values in declaration order (number / string /
// boolean) — each boxed per its metadata type_tag.
console.log(vs[0]);
console.log(vs[2]);
console.log(vs[3]);
