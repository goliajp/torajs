// S251 — Number/String/Boolean(value, ...trailing) trailing-arg
// ignore per ES §21.1.1 / §22.1.1 / §20.3.1. Spec coerces only the
// 1 useful arg (or returns the abrupt default if 0-arg); trailing
// operands type_of'd then dropped at SSA-emit time.

// --- Number(value, ...trailing) ---
console.log(Number(42, 99));
console.log(Number(42, 1, 2, 3));
console.log(Number("3.14", "junk"));
console.log(Number(true, 100));
console.log(Number(null, 1, 2));

// --- String(value, ...trailing) ---
console.log(String(42, 99));
console.log(String("hello", "junk"));
console.log(String(true, 100));
console.log(String(null, 1, 2));
console.log(String([1, 2, 3], "extra"));

// --- Boolean(value, ...trailing) ---
console.log(Boolean(42, 99));
console.log(Boolean("", 1, 2));
console.log(Boolean(0, 1, 2, 3));
console.log(Boolean(null, "junk"));
console.log(Boolean(undefined, true));
