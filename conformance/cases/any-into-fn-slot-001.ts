// An `any` value written into a slot the program typed as a function: a
// function object of any kind (a closure, a bound function, a builtin)
// is stored as it is and called through its own entry. A binding (a
// `let`, a reassigned one, a module global) holds a non-function as it
// is too, as bun does. An element of a function array and a field of an
// object type literal still check the value where it is stored, a
// TypeError for a non-function where bun stores it and throws when it is
// called, so the `store index` and `store field` lines differ from bun
// (checked against the .expected).
function attempt(tag: string, f: () => void): void {
  try {
    f()
    console.log(tag, "ok")
  } catch (e) {
    console.log(tag, e instanceof TypeError)
  }
}
const five: any = 5
const arrow: any = () => console.log("arrow ran")
function base(this: any, x: number): number {
  return x + 1
}
const bound: any = base.bind(null)
const max: any = Math.max

attempt("let arrow", () => {
  const g: () => void = arrow
  g()
})
attempt("let bound", () => {
  const g: (x: number) => number = bound
  console.log(g(1))
})
attempt("let bound no args", () => {
  const g: () => void = bound
  g()
})
attempt("let builtin", () => {
  const h: (a: number, b: number) => number = max
  console.log(h(3, 4))
})
attempt("store let", () => {
  const g: () => void = five
  console.log("stored")
})
attempt("store reassign", () => {
  let g: () => void = arrow
  g = five
  console.log("stored")
})
const fns: (() => void)[] = []
attempt("index arrow", () => {
  fns[0] = arrow
  console.log(fns.length)
})
attempt("store index", () => {
  const bad: (() => void)[] = []
  bad[0] = five
  console.log("stored")
})
attempt("store field", () => {
  const o: { cb: () => void } = { cb: arrow }
  o.cb = five
  console.log("stored")
})
attempt("field bound", () => {
  const o: { cb: (x: number) => number } = { cb: (x: number) => x }
  o.cb = bound
  console.log(o.cb(5))
})
let top: () => void = arrow
try {
  top = five
  console.log("stored")
} catch (e) {
  console.log("store global", e instanceof TypeError)
}
top = arrow
top()
for (const f of fns) f()
console.log(fns.length)
