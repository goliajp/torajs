// V3-18 wedge — class field declaration with no explicit type
// annotation, type inferred from a literal initializer:
//   class C { count = 0; tag = "x"; on = true }
// Per TS spec the field's type is inferred from the init
// expression's contextual type. Subset infers from literal-
// shape (Number / String / Boolean) — the dominant pattern.
// Pre-fix tora's parser bailed at the `=` after the field
// name with 'expected `(` (method) or `:` (field) after `count`'.
//
// Implementation: in the class-member dispatch, add a
// Some(Token::Eq) branch alongside the existing Colon (typed
// field) and LParen (method) branches. Inferred type is
// "number" / "string" / "boolean" depending on the init's
// Expr::Number / Expr::String / Expr::Bool shape; non-literal
// inits still require an explicit ann (would need full type
// inference otherwise).

class P {
  x = 10
  y = "hi"
  z = false
  describe(): string {
    return this.x + ":" + this.y + ":" + this.z
  }
}
let p = new P()
console.log(p.describe())              // 10:hi:false

// Static fields with no type ann.
class Stat {
  static count = 0
  static name_str = "default"
  static enabled = true
  static inc(): void { Stat.count++ }
}
Stat.inc(); Stat.inc(); Stat.inc()
console.log(Stat.count)                // 3
console.log(Stat.name_str)             // default
console.log(Stat.enabled)              // true

// Mixed: typed + untyped fields in same class.
class Mix {
  a: number = 1
  b = 2
  c: string = "ok"
  d = "shorthand"
  sum(): number { return this.a + this.b }
}
let m = new Mix()
console.log(m.sum(), m.c, m.d)         // 3 ok shorthand

// Untyped private/readonly modifier still works.
class M2 {
  private state = 42
  readonly tag = "M2"
  read(): string { return this.tag + ":" + this.state }
}
let m2 = new M2()
console.log(m2.read())                 // M2:42
