// RFC 20260708-typed-arr-oob-read chunk 1 — `string[]` indexed
// read grows a bounds branch: a negative or >= len index answers
// the immortal `undefined` Str sentinel (ES §10.4.3 [[Get]] miss)
// instead of reading a garbage/NULL slot (pre-fix: `s[9]` printed
// `null`, `typeof` said "string" through the static fold, and
// `=== undefined` answered false). Every consumer below rides the
// existing sentinel family (chunks 649-667): print walks the
// "undefined" payload, typeof / strict-eq branch on the address,
// and the chunk 667 is_nullable_str_source Index arm keeps the
// typeof two-state for Array<Str> receivers. In-bounds reads keep
// the direct LoadDyn (the check is dominated-eliminable inside
// `i < arr.length` loops). number[] / boolean[] / nested-heap OOB
// stay unchecked — the RFC's chunks 2-3.

const s: string[] = ["x", "y"];

// const OOB index.
console.log(s[9]);                            // undefined
console.log(typeof s[9]);                     // undefined
console.log(s[9] === undefined);              // true

// in-bounds stays direct.
console.log(s[0]);                            // x
console.log(s[1]);                            // y

// dynamic OOB index.
const i: number = 5;
console.log(s[i]);                            // undefined

// negative index — property miss per §10.4.3.
console.log(s[-1]);                           // undefined

// loop shape: `i <= len` off-by-one reads one past the end.
let seen = 0;
for (let j = 0; j <= s.length; j++) {
  if (s[j] === undefined) { seen = seen + 1; }
}
console.log(seen);                            // 1
