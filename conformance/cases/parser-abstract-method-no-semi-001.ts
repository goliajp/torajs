// V3-18 wedge — abstract method declarations may omit the
// trailing `;` per ES spec (semicolons are optional via ASI
// when the next token naturally starts a new declaration).
// Pre-fix tora's class parser strictly required `;` after the
// signature with the diagnostic
// 'abstract method `<name>` cannot have a body — expected `;`,
// got <next_member>'.
//
// Real-world TS code routinely writes the next class member
// directly after the abstract signature without a `;`:
//   abstract class Shape {
//     abstract area(): number       // ← no semi
//     describe(): string { ... }
//   }
// Bun and tsc both accept this; tora now does too.
//
// Implementation: in parse_class's abstract-method branch,
// the `;` consumption is now optional (consume if present,
// otherwise fall through to the next member). Concrete
// methods still require `{ ... }` so there's no ambiguity.

abstract class Shape {
  abstract area(): number
  abstract perimeter(): number
  describe(): string {
    return "area=" + this.area() + " perim=" + this.perimeter()
  }
}

class Square extends Shape {
  constructor(public side: number) { super() }
  area(): number { return this.side * this.side }
  perimeter(): number { return 4 * this.side }
}

let s = new Square(4)
console.log(s.area())                          // 16
console.log(s.perimeter())                     // 16
console.log(s.describe())                      // area=16 perim=16

// Trailing `;` still works (the wedge only relaxes, never
// rejects the explicit form).
abstract class A {
  abstract m(): string;
  abstract n(): number;
}
class B extends A {
  m(): string { return "m" }
  n(): number { return 42 }
}
let b = new B()
console.log(b.m())                             // m
console.log(b.n())                             // 42

// Mixed: some with `;`, some without.
abstract class Mixed {
  abstract a(): number;
  abstract b(): string
  abstract c(): boolean
}
class MixedImpl extends Mixed {
  a(): number { return 1 }
  b(): string { return "two" }
  c(): boolean { return true }
}
let m = new MixedImpl()
console.log(m.a(), m.b(), m.c())               // 1 two true
