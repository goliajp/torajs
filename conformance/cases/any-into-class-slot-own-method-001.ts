// An object that is no class instance, written through `any` into a
// slot of a class, is copied into a fresh instance when it has the
// class's fields: the instance then answers the class's methods and
// accessors. An object with a property of its own under one of those
// names would lose it in the copy and run the class's member instead,
// so it is refused with a TypeError (bun keeps the object as it is and
// calls its own method; the lines that differ are checked against the
// .expected). An object without such a property is still copied.
class W {
  a = 1
  m(): number {
    return 10
  }
}
class V extends W {
  get g(): number {
    return 3
  }
}
type R = { w: W }

function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, "ok")
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message)
  }
}
const own: any = { a: 2, m() { return 20 } }
const parsed: any = JSON.parse('{"a":3}')
const getter: any = { a: 4, g: 5 }
attempt("let", () => {
  const w: W = own
  console.log(w.a, w.m())
})
attempt("member", () => {
  const r: R = { w: own }
  console.log(r.w.a)
})
attempt("subclass inherited method", () => {
  const v: V = own
  console.log(v.a)
})
attempt("subclass accessor", () => {
  const v: V = getter
  console.log(v.a)
})
attempt("copied", () => {
  const w: W = parsed
  console.log(w.a)
})
attempt("member copied", () => {
  const r: R = { w: parsed }
  console.log(r.w.a)
})
