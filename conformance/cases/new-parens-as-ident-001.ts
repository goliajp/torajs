// rotation 191 — `new (Ident as Ty)(args)` computed callee.
// Before the fix parser rejected any non-Ident inner of `new (…)`,
// which forbade the common escape-hatch idiom `new (Class as any)(m)`
// used to instantiate a class through a widened type. `as` is a
// static-only assertion; the runtime callee IS still the inner
// Ident, so the parser now peels one `Expr::As` before enforcing the
// class-ident requirement.

class Boom extends Error {
  constructor(msg: string) {
    super(msg)
  }
}

const a = new Boom('direct')
console.log(a.message, a instanceof Boom, a instanceof Error)

const b = new (Boom as any)('via-as-any')
console.log(b.message, b instanceof Boom, b instanceof Error)

// The `as` unwrap also accepts a nominal-type assertion (still a
// static-only cast — the class ident inside carries the ctor).
class Point {
  constructor(
    public x: number,
    public y: number,
  ) {}
}
const c = new (Point as any)(3, 4)
console.log(c.x, c.y, c instanceof Point)
