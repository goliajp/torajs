// W-J Phase C1 — Object.keys / Object.getOwnPropertyNames on an
// `any`-typed value that carries a static-layout struct cell. tora
// lowers `Object.keys(o)` to a compile-time field-name literal when
// `o`'s static type is a struct, but an `any` binding hides that, so
// SSA-lower routes to the struct_enum runtime arm: read the
// instance's class_tag, look the layout up in __torajs_class_layouts
// (torajs-structmeta Phase A4 helpers), and walk the field names in
// declaration order. `keys` and `getOwnPropertyNames` coincide for a
// struct (no prototype chain, no array `length`), so both surfaces
// share the arm.
//
// `.join(",")` keeps the comparison on the string contents, not the
// array's nested-print formatting.
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

// Object.keys — field names in declaration order.
console.log(Object.keys(p).join(","));

// Object.getOwnPropertyNames — same shape for a struct.
console.log(Object.getOwnPropertyNames(p).join(","));

// Field count is observable through the array length.
console.log(Object.keys(p).length);
