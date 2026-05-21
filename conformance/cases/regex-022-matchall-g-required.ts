// P9.4 follow-up — `s.matchAll(re)` throws when re is a RegExp lacking
// the `g` flag, per ES spec §22.1.3.13. Pre-fix tora silently iterated
// regardless of flag (over-permissive; bun-parity gap on test262
// entries probing the gate).
//
// Note on caught-type shape: bun raises a real TypeError instance;
// tora's catchable path raises a plain string until P7.4-a-2 wires
// the native-error-reg startup registration. So this fixture asserts
// the throw FIRES (try/catch sees something) without inspecting the
// caught object's class — that side of bun-parity is a separate
// substrate gap tracked under P7.4.

// Path A — g flag present → works as before
const g = /a/g
const matches = 'aXaXa'.matchAll(g)
console.log('with g:')
for (const m of matches) console.log(m[0])

// Path B — no g flag → must throw
let thrown_b = false
try {
  const ng = /a/
  'aXa'.matchAll(ng)
} catch (e) {
  thrown_b = true
}
console.log('no-g throws:', thrown_b)

// Path C — i flag (no g) → still throws
let thrown_c = false
try {
  const i = /a/i
  'aA'.matchAll(i)
} catch (e) {
  thrown_c = true
}
console.log('i-only throws:', thrown_c)

// Path D — y+g together → g present, no throw
const yg = /a/gy
yg.lastIndex = 0
const ygm = 'aaa'.matchAll(yg)
console.log('y+g:')
for (const m of ygm) console.log(m[0])
