// S253 — Number.<static>(value, ...trailing) trailing-arg ignore per
// ES §21.1.2.{2,3,4,5,12,13}: Number.{isFinite,isNaN,isInteger,
// isSafeInteger,parseInt,parseFloat} read only the declared positional
// args; trailing operands type_of'd then dropped at SSA-emit time.

// --- Number.isFinite(value, ...trailing) ---
console.log(Number.isFinite(1, 99));
console.log(Number.isFinite(Infinity, 1, 2));
console.log(Number.isFinite("3", "junk", true));

// --- Number.isNaN(value, ...trailing) ---
console.log(Number.isNaN(NaN, 99));
console.log(Number.isNaN(42, 1, 2));
console.log(Number.isNaN("foo", "junk"));

// --- Number.isInteger(value, ...trailing) ---
console.log(Number.isInteger(42, 99));
console.log(Number.isInteger(1.5, 1, 2));
console.log(Number.isInteger("42", "junk", true));

// --- Number.isSafeInteger(value, ...trailing) ---
console.log(Number.isSafeInteger(100, 99));
console.log(Number.isSafeInteger(1, "junk", true));

// --- Number.parseInt(str, radix, ...trailing) ---
console.log(Number.parseInt("ff", 16, 99));
console.log(Number.parseInt("100", 2, 1, 2, 3));
console.log(Number.parseInt("42", 10, "junk"));

// --- Number.parseFloat(str, ...trailing) ---
console.log(Number.parseFloat("3.14", 99));
console.log(Number.parseFloat("2.5e2", 1, 2, 3));
