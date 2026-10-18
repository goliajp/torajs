// An object owning a class's method name is refused wherever the cast
// reaches a class node, not only at the slot itself: an element of an
// array field, an optional slot (`W | null`, `W | undefined`, `w?: W`),
// and a link of a type that contains itself. Each refusal has a
// counterpart without the name that is copied. bun keeps the object as
// it is, so the refused lines differ from bun and are checked against
// the .expected; the copied lines print fields only and match bun.
class W {
  a = 1
  m(): number {
    return 10
  }
}
class Holder {
  ws: W[] = []
  n = 0
}
class Opt {
  w: W | null = null
  n = 0
}
class N {
  v = 0
  next: N | null = null
  m(): number {
    return 1
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
function holder(raw: any): number {
  const h: Holder = raw
  return h.ws.length + h.n + h.ws[0].a
}
function opt(raw: any): number {
  const o: Opt = raw
  return o.w !== null ? o.w.a : -1
}
function sum(n: N | null): number {
  let s = 0
  while (n !== null) {
    s += n.v
    n = n.next
  }
  return s
}
function chain(raw: any): number {
  const n: N = raw
  return sum(n)
}
const own: any = { a: 2, m() { return 20 } }
const parsed: any = JSON.parse('{"a":3}')

attempt("array element", () => console.log(holder(JSON.parse('{"n":1,"ws":[{"a":2,"m":5}]}'))))
attempt("array element copied", () => console.log(holder(JSON.parse('{"n":1,"ws":[{"a":2}]}'))))
attempt("nullable field", () => console.log(opt(JSON.parse('{"n":1,"w":{"a":2,"m":5}}'))))
attempt("nullable field copied", () => console.log(opt(JSON.parse('{"n":1,"w":{"a":2}}'))))
attempt("nullable field null", () => console.log(opt(JSON.parse('{"n":1,"w":null}'))))
attempt("self link", () => {
  console.log(chain(JSON.parse('{"v":1,"next":{"v":2,"next":{"v":3,"m":5,"next":null}}}')))
})
attempt("self link copied", () => {
  console.log(chain(JSON.parse('{"v":1,"next":{"v":2,"next":{"v":3,"next":null}}}')))
})
attempt("let or null", () => {
  const w: W | null = own
  console.log(w !== null ? w.a : 0)
})
attempt("let or undefined", () => {
  const w: W | undefined = own
  console.log(w !== undefined ? w.a : 0)
})
attempt("let or null copied", () => {
  const w: W | null = parsed
  console.log(w !== null ? w.a : 0)
})
attempt("optional member", () => {
  const r: { w?: W } = { w: own }
  console.log(r.w !== undefined ? r.w.a : 0)
})
attempt("optional member copied", () => {
  const r: { w?: W } = { w: parsed }
  console.log(r.w !== undefined ? r.w.a : 0)
})
