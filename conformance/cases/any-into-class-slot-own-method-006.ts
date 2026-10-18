// An object holds a class's method name for the cast into the class
// when it owns it or inherits it from a prototype ahead of
// %Object.prototype%: the copy would answer the class's member where
// the object answers that prototype's. A symbol-keyed method counts
// under its symbol. What the object inherits from %Object.prototype%
// the copy inherits too, so a class declaring `toString` still takes a
// plain object, and so does a null-prototype object or one whose
// prototype holds none of the names. bun keeps the object as it is, so
// the refused lines differ from bun and are checked against the
// .expected; the other lines print fields only and match bun.
class W {
  a = 1
  m(): number {
    return 10
  }
  *[Symbol.iterator](): Generator<number> {
    yield 1
  }
}
class T {
  a = 1
  toString(): string {
    return "T"
  }
}

function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, "ok")
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message)
  }
}
attempt("inherited", () => {
  const o: any = Object.create({ m() { return 20 } })
  o.a = 2
  const w: W = o
  console.log(w.a)
})
attempt("inherited two links up", () => {
  const o: any = Object.create(Object.create({ m() { return 20 } }))
  o.a = 2
  const w: W = o
  console.log(w.a)
})
attempt("symbol key", () => {
  const o: any = { a: 2, *[Symbol.iterator]() { yield 2 } }
  const w: W = o
  console.log(w.a)
})
attempt("object prototype name", () => {
  const o: any = { a: 3 }
  const t: T = o
  console.log(t.a)
})
attempt("null prototype", () => {
  const o: any = Object.create(null)
  o.a = 4
  const w: W = o
  console.log(w.a)
})
attempt("prototype without the names", () => {
  const o: any = Object.create({ z: 1 })
  o.a = 5
  const w: W = o
  console.log(w.a)
})
