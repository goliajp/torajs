// rotation 507 (506-04) — the W5 counter carve-out (an Add/Sub with an
// integer-literal step passes through unmarked) is a trip-count
// argument and is only sound for SMALL steps: `t += 2^53-1` leaves the
// exact-i64 window on the second trip. Big literal steps now mark
// growth and ride the float_demote guard, so the accumulator side-exits
// into the f64 loop and rounds exactly like bun; small steps keep the
// unmarked i64 counter.
let t = 0;
for (let i = 0; i < 2000; i++) t += 9007199254740991;
console.log(t);
let d = 0;
for (let i = 0; i < 2000; i++) d -= 9007199254740991;
console.log(d);
// a step just past 2^32 over 3M trips: the exact i64 sum is odd past
// 2^53, so the f64 accumulation must round like bun's
let u = 0;
for (let i = 0; i < 3000000; i++) u += 4294967297;
console.log(u);
let v = 0;
for (let i = 0; i < 3000000; i++) v = v + 4294967297;
console.log(v);
// small literal steps stay on the unmarked counter lane
let s = 0;
for (let i = 0; i < 100000; i++) s += 7;
console.log(s);
let w = 0;
for (let i = 0; i < 100000; i++) w = w - 32;
console.log(w);
let x = 0;
for (let i = 0; i < 100000; i++) x += 33;
console.log(x);
