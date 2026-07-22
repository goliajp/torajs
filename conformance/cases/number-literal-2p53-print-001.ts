// Integer literals beyond 2^53 are doubles in JS: printing is
// shortest-roundtrip, toFixed stays exact (rotation 184 — the old
// 2^63 I64-tier bound printed the exact decimal instead).
console.log(1000000000000000128);
console.log((1000000000000000128).toString());
console.log((1000000000000000128).toFixed(0));
console.log(-1000000000000000128);
console.log(9007199254740992);
console.log(9007199254740993);
console.log(123456789012345678 + 1);
console.log(1e16);
console.log(1e21);
