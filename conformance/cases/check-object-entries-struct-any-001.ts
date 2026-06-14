// W-J Phase C3 — Object.entries on an `any`-typed value that carries a
// static-layout struct cell. tora unfolds `Object.entries(o)` at
// compile time when `o`'s static type is a struct; an `any` binding
// hides that, so SSA-lower routes to the struct_enum runtime walker,
// which builds an Array<Array<Any>> of [name, value] pairs (each inner
// is a length-2 Arr<Any>). Field values are boxed per their metadata
// type_tag, so a heterogeneous struct round-trips.
//
// Indexed reads keep the comparison on individual scalars (the nested
// Array<Array<Any>> print / join formatting is exercised elsewhere).
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
const es = Object.entries(p);

// Pair count.
console.log(es.length);

// [name, value] pairs in declaration order.
console.log(es[0][0]);
console.log(es[0][1]);
console.log(es[2][0]);
console.log(es[2][1]);
console.log(es[3][1]);
