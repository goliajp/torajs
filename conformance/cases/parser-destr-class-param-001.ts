// V3-18 wedge — destructuring patterns in class method and
// class constructor parameter positions, the third leg of the
// destr-fn-param wedge family. After 0ba77d4 (parse_fn) and
// 3534506 (parse_arrow_fn), `parse_param_list` and
// `parse_ctor_param_list` were the remaining sites that
// rejected `[a, b]` / `{ x, y }` patterns at param position.
//
// Implementation: parse_param_list now returns
// `(Vec<Param>, Vec<Stmt>)` — the second element is the
// destr-let vec to prepend to the body. parse_ctor_param_list
// returns `(params, promoted_props, destr_lets)` — same
// pattern, with the side-table for TS parameter-property
// shorthand kept intact. All three callers (object-literal
// method shorthand, class-member method, function-expression)
// were updated to thread destr_lets into the body.
//
// Deliberate constraint: a ctor destr param can't carry
// visibility / readonly modifiers — a binding pattern has
// no single field-name to promote into a class field. The
// parser rejects the combo with a clear diagnostic.

// Class method: array destr param.
class A {
  greet([a, b]: number[]): number { return a + b }
  cube([x, y, z]: number[]): number { return x * y * z }
}
let a = new A()
console.log(a.greet([3, 4]))                   // 7
console.log(a.greet([10, 20]))                 // 30
console.log(a.cube([2, 3, 4]))                 // 24

// Class method: object destr param + rename.
class B {
  named({ first, last }: { first: string, last: string }): string {
    return first + " " + last
  }
  scaled({ x: u, y: v }: { x: number, y: number }, k: number): number {
    return (u + v) * k
  }
}
let b = new B()
console.log(b.named({ first: "Alice", last: "Smith" }))
                                               // Alice Smith
console.log(b.scaled({ x: 3, y: 4 }, 10))      // 70

// Constructor: array destr param.
class P {
  v: number = 0
  constructor([a, b]: number[]) { this.v = a + b }
}
let p = new P([3, 4])
console.log(p.v)                               // 7
let p2 = new P([100, 200])
console.log(p2.v)                              // 300

// Constructor: object destr param.
class Pt {
  x: number = 0
  y: number = 0
  constructor({ x, y }: { x: number, y: number }) { this.x = x; this.y = y }
  sum(): number { return this.x + this.y }
}
let pt = new Pt({ x: 5, y: 7 })
console.log(pt.sum())                          // 12
console.log(pt.x, pt.y)                        // 5 7

// Mixed: ident + destr params in a class method.
class M {
  combine(prefix: string, [a, b]: number[]): string {
    return prefix + ": " + (a + b)
  }
}
let m = new M()
console.log(m.combine("sum", [3, 4]))          // sum: 7
console.log(m.combine("sum", [10, 20]))        // sum: 30
