// Cluster #4 follow-up — a bare top-level fn passed to an Object /
// Reflect namespace-static call wraps to a closure cell
// (ast_collect_fn_closure Object/Reflect-arg axis), so the
// reflection kernels see a boxable receiver and answer spec meta.
function inner(a: number, b: number) {
  return a + b
}
let d = Object.getOwnPropertyDescriptor(inner, "caller")
console.log(d)
let dl = Object.getOwnPropertyDescriptor(inner, "length")
console.log(dl)
console.log(Object.getOwnPropertyDescriptor(inner, "name"))
function solo() {
  return 1
}
let ds = Object.getOwnPropertyDescriptor(solo, "nope")
console.log(ds === undefined)
