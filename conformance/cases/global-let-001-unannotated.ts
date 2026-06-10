// K.3b — a top-level `let`/`const` without a type annotation must
// still promote to a data-global slot when a named fn body reads it.
// Before the fix, lowering skipped un-annotated declarations and the
// fn-body read aborted with `ssa-lower: unknown ident`.

let n = 10
function f(): string {
  return "x".repeat(n)
}
console.log(f().length)

let b = true
function g(): boolean {
  return !b
}
console.log(g())

const s = "abc"
function h(): number {
  return s.length
}
console.log(h())

let count = 0
function bump(): void {
  count = count + 1
}
bump()
bump()
console.log(count)
