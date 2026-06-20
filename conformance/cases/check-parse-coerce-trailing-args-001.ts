// S252 — parseInt/parseFloat/isNaN/isFinite(value, ...trailing)
// trailing-arg ignore per ES §19.2.3 / §19.2.4 / §19.2.5. Spec reads
// only the declared positional args (parseInt up to 2, others up to
// 1); trailing operands type_of'd then dropped at SSA-emit time.

// --- parseInt(str, radix, ...trailing) ---
console.log(parseInt("ff", 16, 99));
console.log(parseInt("100", 2, 1, 2, 3));
console.log(parseInt("42", 10, "junk"));
console.log(parseInt("ff", 16, true, false));

// --- parseFloat(str, ...trailing) ---
console.log(parseFloat("3.14", 99));
console.log(parseFloat("2.5e2", 1, 2, 3));
console.log(parseFloat("0.5", "junk", true));

// --- isNaN(value, ...trailing) ---
console.log(isNaN(NaN, 99));
console.log(isNaN(42, 1, 2));
console.log(isNaN("3.14", "junk"));
console.log(isNaN(0 / 0, true));

// --- isFinite(value, ...trailing) ---
console.log(isFinite(1, 99));
console.log(isFinite(Infinity, 1, 2));
console.log(isFinite("3.14", "junk"));
console.log(isFinite(NaN, true, false));
