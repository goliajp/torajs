// L3a-8 — int32 semantics for shift/bitwise operators (JS ToInt32 /
// ToUint32, spec §13.9 / §13.12): 2^31 sign wrap, negative `>>`,
// zero-fill `>>>`, `<<` overflow truncation, shift-count masking, and
// `~` on out-of-int32 values. Constant shapes — egraph const-fold and
// the runtime path must agree with bun on every line.
console.log(1 << 31);
console.log(1024 << 30);
console.log(1 << 32);
console.log(16 >> 33);
console.log(-8 >> 1);
console.log(-1 >> 31);
console.log(-1 >>> 0);
console.log(-8 >>> 1);
console.log(4294967296 | 0);
console.log(123456789012 & -1);
console.log(~5);
console.log(~4294967295);
console.log(5 ^ 3);
console.log((2147483647 + 1) | 0);
console.log(3000000000 ^ 0);
