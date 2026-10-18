// The same refusal at the other slots an `any` value crosses into a
// class through: a written return type, a class field and a field of
// a nested object type. An object owning only names the class does
// not answer from its prototype (other keys, the name of a private
// method) is still copied. bun keeps the object as it is in every
// slot; the refused lines differ and are checked against the
// .expected.
class W {
  a = 1
  #p(): number {
    return 1
  }
  m(): number {
    return 10
  }
  q(): number {
    return this.#p()
  }
}
class H {
  w: W
  constructor(w: any) {
    this.w = w
  }
}
type N = { inner: { w: W } }

function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, "ok")
  } catch (e) {
    console.log(tag, e instanceof TypeError, (e as Error).message)
  }
}
function give(v: any): W {
  return v
}
const own: any = { a: 2, m() { return 20 } }
const other: any = { a: 5, n: 6, p: 7 }
attempt("return", () => console.log(give(own).a))
attempt("field", () => console.log(new H(own).w.a))
attempt("nested", () => {
  const n: N = { inner: { w: own } }
  console.log(n.inner.w.a)
})
attempt("other keys", () => console.log(give(other).a))
attempt("other keys field", () => console.log(new H(other).w.a))
