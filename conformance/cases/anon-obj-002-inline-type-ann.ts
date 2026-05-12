// V3-18 P2.4.c.2 — inline object type annotation `{ x: T; y: U }`
// in fn params, ret types, and let bindings. Pre-fix the parser
// hard-rejected with "expected type name, got LBrace".
//
// Implementation: parser encodes inline obj as
// `__inlobj(name1:T1|name2:T2|...)`. check.rs decodes to
// Type::Struct; ssa_lower aliases `__inlobj(...)` to its
// existing `__struct(...)` parser to register / dedupe layouts.

function getX(o: { x: number }): number { return o.x }
console.log(getX({ x: 99 }))                   // 99

function getXY(o: { x: number; y: number }): number { return o.x + o.y }
console.log(getXY({ x: 10, y: 20 }))           // 30

// Mixed field types.
function describe(o: { name: string; age: number }): string {
  return o.name + " is " + o.age
}
console.log(describe({ name: "Alice", age: 30 }))    // Alice is 30

// Inline ret type.
function makePoint(x: number, y: number): { x: number; y: number } {
  return { x, y }
}
let p = makePoint(3, 4)
console.log(p.x, p.y)                          // 3 4

// Type alias still works (no regression).
type Pt = { x: number; y: number }
let q: Pt = { x: 1, y: 2 }
console.log(q.x, q.y)                          // 1 2
