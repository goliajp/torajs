// A spread-carrying call whose callee declares a default used to keep
// the loud reject, because the dynamic lane has no way to substitute a
// default at the call site. But a default that already moved INTO the
// body is not waiting for a call-site substitution: the guard spliced
// there fires on whatever the relay pads, and that is exactly
// undefined. Reading the skip off "has a default" put every converted
// callee on the reject too — `g(...a)` on `function g(x = 5, ...r)`
// answered `box_to_any element type FnSig`.

function g(x = 5, ...r) {
  return "g:" + x + ":" + r.length
}
const a = [1, 2, 3]
console.log(g(...a))

// the default really fires when the source runs out
const empty: any[] = []
console.log(g(...empty))
const one = [7]
console.log(g(...one))

// a fixed prefix with a defaulted tail, no rest
function d(p, q = 5) {
  return "d:" + p + ":" + q
}
console.log(d(...[1]), d(...[1, 9]))

// every position defaulted
function e(p = 1, q = 2, ...r) {
  return "e:" + p + ":" + q + ":" + r.length
}
console.log(e(...empty), e(...[7]), e(...[7, 8, 9]))

// a default that reads a prior parameter — §9.2 binds it in the
// callee's own scope, which is where the guard already lives
function h(k, m = k + 1, ...r) {
  return "h:" + k + ":" + m + ":" + r.length
}
console.log(h(...[3]), h(...[3, 30]), h(...[3, 30, 300]))

// a string default, and an explicit undefined element still defaulting
function s(p = "b", ...r) {
  return "s:" + p + ":" + r.length
}
console.log(s(...empty), s(...["z"]), s(...[undefined, 1]))

// the spread source is not disturbed: §10.4.2 builds a fresh array
function keep(p = 0, ...r) {
  r.push(9)
  return r.length
}
const src = [1, 2, 3]
console.log(keep(...src), src.length)

// a non-trailing spread on a defaulted callee
console.log(d(...[1], 4))
