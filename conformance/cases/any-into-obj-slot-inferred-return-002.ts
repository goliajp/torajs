// A function whose return type is inferred from an object literal
// returns the layout the literal has after a field of it is stored a
// value of any kind: the caller reads the field as that value, through
// a plain function, a chain of calls, an arrow, a method (overridden
// or not), an array or another literal around it.
class X {
  n = 5
}

function mk(v: any) {
  const o = { t: { s: "a" } }
  o.t = v
  return o
}
function outer(v: any) {
  return mk(v)
}
const arrow = (v: any) => {
  const o = { t: { s: "a" } }
  o.t = v
  return o
}
function wrap(v: any) {
  const o = { t: { s: "a" } }
  o.t = v
  return [o]
}
function deep(v: any) {
  const o = { t: { s: "a" } }
  o.t = v
  const p = { q: o }
  return p
}
class B {
  m(v: any) {
    const o = { t: { s: "a" } }
    o.t = v
    return o
  }
}
class C extends B {
  m(v: any) {
    return { t: { s: "c" } }
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

attempt("class instance", () => {
  const o = mk(new X() as any)
  console.log(typeof o.t.s)
})
const a = mk(7 as any)
console.log(typeof a.t, a.t)
const b = outer({ s: "zz" } as any)
console.log(b.t.s)
const c = mk("str" as any)
console.log(typeof c.t, c.t)
const d = arrow(8 as any)
console.log(typeof d.t, d.t)
const w = wrap(9 as any)
console.log(typeof w[0].t, w[0].t)
const e = deep(true as any)
console.log(typeof e.q.t, e.q.t)
const xs: B[] = [new B(), new C()]
for (const x of xs) {
  const r = x.m(4 as any)
  console.log(typeof r.t, JSON.stringify(r.t))
}
