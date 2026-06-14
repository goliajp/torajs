// W-J Phase B — Object.getOwnPropertyDescriptor on a static-layout
// struct (class instance) cell. Pre-W-J the reflect.rs cell arm only
// recognised Tag::DynObj (dynamic-property objects); a Tag::Obj class
// instance fell through to `undefined`. Phase B routes Tag::Obj cells
// to the struct arm, which reads the field via the class_layouts
// metadata (torajs-structmeta Phase A4 helpers) and builds the full
// { value, writable, enumerable, configurable } data descriptor.
//
// Struct fields are ordinary data properties → all three attribute
// flags are true (ECMA-262 §10.4.x). Descriptor field access uses
// `as any` + member read (printing the whole descriptor object goes
// through the [object] fallback, a separate Tag::Obj print trunk).
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
const p = new Point(3, 7, "origin", true);

// number field — full 4-key descriptor.
const dx = Object.getOwnPropertyDescriptor(p, "x") as any;
console.log(dx.value, dx.writable, dx.enumerable, dx.configurable);

// string field value.
const dl = Object.getOwnPropertyDescriptor(p, "label") as any;
console.log(dl.value);

// boolean field value.
const da = Object.getOwnPropertyDescriptor(p, "active") as any;
console.log(da.value);

// absent key → undefined.
const dm = Object.getOwnPropertyDescriptor(p, "missing");
console.log(dm === undefined);
