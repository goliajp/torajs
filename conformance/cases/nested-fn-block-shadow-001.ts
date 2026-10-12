// A nested function declaration is lifted and renamed; every scope
// that rebinds its name keeps reading its own binding.

function n() {
  function f() { return 1 }
  { const f = () => 9; console.log(f()) }
  return f()
}
console.log(n())

function a(): number {
  function f(): number { return 1 }
  let out = 0
  { let f = (): number => 2; out += f() }
  { class f { v(): number { return 3 } } out += new f().v() }
  { function f(): number { return 4 } out += f() }
  for (const f of [5]) { out += f }
  for (let f = 6; f < 7; f++) { out += f }
  try { throw 7 } catch (f) { out += f as number }
  switch (out) { default: { const f = 8; out += f } }
  const g = (f: number): number => f + 100
  out += g(9)
  const h = (): number => { const f = 10; return f }
  out += h()
  const k = (): number => { { const f = 11; return f } }
  out += k()
  return out + f()
}
console.log(a())

function b() {
  function f() { return "outer" }
  const o = { f }
  const p = { f: 1 }
  console.log(o.f(), p.f, typeof o.f)
  const r = () => { const s = () => f(); return s() }
  console.log(r())
  switch (1) { case 1: { console.log(f()) } }
  return f()
}
console.log(b())

function c() {
  function f() { return 1 }
  const m = [0].map(() => { { const f = () => 9; return f() } })
  console.log(m[0])
  const t = 0
  switch (t) { case 0: let f = () => 5; console.log(f()); break }
  return f()
}
console.log(c())

const q = function () {
  function g() { return 2 }
  { const g = () => 7; console.log(g()) }
  return g()
}
console.log(q())

function cc() {
  try { throw 1 } catch (e) {
    function h() { return 3 }
    { const h = () => 6; console.log(h()) }
    console.log(h(), e)
  }
}
cc()

function dd() {
  function f() { return 4 }
  function g(f: number) { return f + 1 }
  console.log(g(10), f())
  function r(n: number): number { return n <= 0 ? f() : r(n - 1) }
  console.log(r(2))
  function s() { function f() { return 5 } return f() }
  console.log(s(), f())
}
dd()

function cl() {
  function f() { return 1 }
  class C { m() { return f() } }
  { class D { m() { const f = () => 2; return f() } } console.log(new D().m()) }
  return new C().m()
}
console.log(cl())

function lp() {
  function f() { return 3 }
  let s = 0
  for (const f of [10, 20]) s += f
  for (let f = 0; f < 2; f++) s += f
  let i = 0
  while (i < 1) { const f = 100; s += f; i++ }
  if (s > 0) { const f = () => 1000; s += f() }
  return s + f()
}
console.log(lp())

function nm() {
  function f() { return "decl" }
  const e = function f(n: number): string { return n > 0 ? f(n - 1) : "expr" }
  return e(2) + " " + f()
}
console.log(nm())
