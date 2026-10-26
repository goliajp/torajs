// An object written through `any` into a binding annotated with a
// class is held as it is, whatever it owns or inherits under the
// class's method names: a method it owns, one a prototype ahead of
// %Object.prototype% gives it, a symbol-keyed one, and a null-prototype
// object. Its members are read off the object itself.
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
