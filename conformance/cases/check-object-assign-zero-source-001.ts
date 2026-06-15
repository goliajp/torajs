// S127-6 — Object.assign(target) with no sources per spec §20.1.2.1
// step 4: "If only one argument was passed, return To(target)." Returns
// target itself unchanged. Pre-fix S127-3 hard-coded `args.len() < 2`
// so `Object.assign(o)` rejected with "expects at least 2 args" — spec
// allows the identity form.
type S = { x: number; y: number }
const o: S = { x: 1, y: 2 }
const r = Object.assign(o)
console.log("r.x =", r.x)
console.log("r.y =", r.y)
// r and o reference the same struct (identity per spec)
o.x = 99
console.log("after o.x=99, r.x =", r.x)
// Statement-level discard
Object.assign(o)
console.log("post-discard o.y =", o.y)
