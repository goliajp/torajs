// V3-18 wedge — TS interface / type-decl method-shape field:
//   interface Shape {
//     area(): number;
//     describe(prefix: string): string;
//   }
// Per TS spec §3.7, a method-shape field `m(p: T): R` is
// equivalent to `m: (p: T) => R` — a function-typed property.
// Pre-fix tora's parser bailed at the type-body field reader
// with 'expected `:` after field name `area`, got LParen'.
//
// Implementation: parse_type_decl_field detects `(` after the
// field name and parses a parenthesized param-type list +
// optional `: R` return type, synthesizing the
// `__fn(P|...)->R` annotation string used elsewhere in tora.
//
// Subset limitation: a struct-typed binding holding a fn-shape
// field still cannot be CALLED via member access (the
// runtime-side `obj.f()` lowering for struct-fn fields is not
// yet wired). The wedge primarily unblocks the *parse* of
// real-world interface declarations matched by class methods
// — `class C implements I` already worked once the parser no
// longer bailed.

interface Shape {
  area(): number;
}

// Class implementing the interface — the actual call goes
// through the class-method dispatch path, not struct-field.
class Square implements Shape {
  constructor(public side: number) {}
  area(): number { return this.side * this.side }
}
let s = new Square(5)
console.log(s.area())                   // 25

interface Vec {
  add(o: Vec): Vec;
  scale(k: number): Vec;
}
class V2 implements Vec {
  constructor(public x: number, public y: number) {}
  add(o: V2): V2 { return new V2(this.x + o.x, this.y + o.y) }
  scale(k: number): V2 { return new V2(this.x * k, this.y * k) }
}
let v = new V2(3, 4)
let w = v.add(new V2(1, 2)).scale(2)
console.log(w.x, w.y)                   // 8 12

// Generic interface with method.
interface Container<T> {
  get(): T;
}
class IntBox implements Container<number> {
  constructor(public v: number) {}
  get(): number { return this.v }
}
let b = new IntBox(99)
console.log(b.get())                    // 99
