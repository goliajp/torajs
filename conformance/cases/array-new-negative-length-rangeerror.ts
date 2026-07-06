// RC-4 F5 (RFC 20260706-test262-bug-corpus) — `new Array(len)` with
// len outside [0, 2^32-1] throws RangeError per ES §23.1.2.1 step
// 4.b. Pre-fix a negative len arrived at __torajs_arr_alloc_any_filled
// as a huge u64 (0xFFFF..FF), the total-bytes computation overflowed,
// malloc failed, and the header write SIGSEGVd. Message matches
// bun/JSC. test262 built-ins/Array/length/S15.4.2.2_A2.2_T1 covers
// this (also the 2^32 / 2^32+1 upper faces).

try {
  let a = new Array(-1)
  console.log('no-throw', a.length)
} catch (e) {
  console.log('caught', e instanceof RangeError, (e as Error).message)
}

// Call form desugars to the same Construct slot (§23.1.1.1).
try {
  let b = Array(-5)
  console.log('no-throw', b.length)
} catch (e) {
  console.log('caught', e instanceof RangeError)
}

// Above the 2^32-1 ceiling — same RangeError.
try {
  let c = new Array(4294967296)
  console.log('no-throw', c.length)
} catch (e) {
  console.log('caught', e instanceof RangeError)
}

// Legal lengths keep working.
let ok = new Array(3)
console.log(ok.length)
