// Cluster #4 (test262) — calling a member read the per-family
// tables answer with their catch-all Any: the checker's general
// tail admits the call (any absorbs it), the receiver boxes at the
// any-lane boundary, and a bare top-level fn receiver wraps to a
// closure cell (ast_collect_fn_closure member-call-receiver axis).
function inner() {
  return 1
}
console.log(inner.hasOwnProperty("caller"))
console.log(inner.propertyIsEnumerable("name"))
const arr = [0, 1]
console.log(arr.hasOwnProperty("0"))
console.log(arr.hasOwnProperty("5"))
const s = "hi"
console.log(s.hasOwnProperty("0"))
console.log((5).hasOwnProperty("x"))
const o = { k: 1 }
console.log(o.hasOwnProperty("k"))
const f2 = function () {
  return 2
}
console.log(f2.hasOwnProperty("length"))
