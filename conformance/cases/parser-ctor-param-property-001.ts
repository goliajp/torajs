// V3-18 wedge — TS constructor parameter-property shorthand:
//   class P { constructor(public x: number, private y: string) {} }
// per TS handbook §"Parameter Properties". The compiler is
// expected to (a) auto-declare an instance field of the same
// name and type, (b) inject `this.<n> = <n>` at the start of
// the ctor body. Pre-fix tora's parser bailed with 'expected
// `,` or `)` in params, got Ident("x")'.
//
// Subset: visibility (`public` / `private` / `protected`) and
// `readonly` modifiers are accepted, in any order before the
// param name. Default value `= expr` and optional `?:` still
// work on promoted params.

class Point {
  constructor(public x: number, public y: number) {}
  distance(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y)
  }
}
let p = new Point(3, 4)
console.log(p.x, p.y)                  // 3 4
console.log(p.distance())              // 5

class User {
  constructor(
    readonly id: number,
    public name: string,
    private age: number,
  ) {}
  describe(): string {
    return this.name + "(" + this.id + ")"
  }
}
let u = new User(7, "alice", 30)
console.log(u.id, u.name)              // 7 alice
console.log(u.describe())              // alice(7)

// Mix: regular param + promoted param. Body can still
// reference both `this.size` (promoted) and `label`
// (regular).
class Box {
  filler: string
  constructor(public size: number, label: string) {
    this.filler = label
  }
}
let bx = new Box(10, "Y")
console.log(bx.size, bx.filler)        // 10 Y

// All four modifier combinations.
class Quad {
  constructor(
    public a: number,
    private b: number,
    protected c: number,
    readonly d: number,
  ) {}
  sum(): number { return this.a + this.b + this.c + this.d }
}
let q = new Quad(1, 2, 3, 4)
console.log(q.sum())                   // 10
