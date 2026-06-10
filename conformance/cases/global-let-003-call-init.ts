// K.3b checker mirror — a top-level `let` initialized by a call (no
// annotation) must be visible to named fn bodies. Before the fix the
// checker's global pre-pass only registered literal-init or annotated
// declarations, so `f` below failed with `unknown identifier y` even
// though fn signatures are hoisted and `g()` types fine.

function g(): number {
  return 7
}
let y = g()
function f(): number {
  return y * 2
}
console.log(f())

function mkTag(): string {
  return "ab".concat("c")
}
const tag = mkTag()
function tagLen(): number {
  return tag.length
}
console.log(tagLen())
