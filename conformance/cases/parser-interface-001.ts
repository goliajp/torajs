// V3-18 wedge — TS `interface X { ... }` declaration. Per TS
// spec §3.7, interfaces declare a structural type; the subset
// treats them as alias for `type X = { ... }` since tora's
// type system is already structural. Pre-fix tora's parser
// bailed at `interface` (which was a plain Ident) followed by
// the class-name shape, hitting unexpected-token errors.
//
// Implementation: stmt-level dispatch routes `interface X` to
// a new parse_interface_decl that mirrors parse_type_decl
// (name + optional <type-params> + optional extends clause +
// `{ <fields> }` body), returning Stmt::TypeDecl so check.rs /
// ssa_lower don't need separate handling.
//
// Subset limitation: `extends Foo, Bar` is parsed but the
// parent fields are NOT merged. Declaration-merging (multiple
// `interface X {}` decls in the same scope) is also not yet
// supported. Method-shape fields like `m(): T` use the
// existing inline-fn-type encoding; calling a function-typed
// field is the same as a free fn so it works the same way.

interface Point {
  x: number;
  y: number;
}
let p: Point = { x: 3, y: 4 }
console.log(p.x, p.y)                  // 3 4

// Generic interface — same shape as type-decl<...>.
interface Vec3<T> {
  x: T;
  y: T;
  z: T;
}
let v: Vec3<number> = { x: 1, y: 2, z: 3 }
console.log(v.x + v.y + v.z)           // 6

// Nullable field.
interface MaybeName {
  name: string | null;
}
let m1: MaybeName = { name: "alice" }
let m2: MaybeName = { name: null }
console.log(m1.name)                   // alice
console.log(m2.name)                   // null

// Interface with array-of-T field.
interface Bag {
  tags: string[];
  size: number;
}
let b: Bag = { tags: ["ts", "rust"], size: 2 }
console.log(b.tags.length, b.size)     // 2 2
