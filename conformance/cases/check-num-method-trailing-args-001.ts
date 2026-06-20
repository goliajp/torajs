// S254 — Number.{toFixed,toExponential,toPrecision}(digits, ...trailing)
// trailing-arg ignore per ES §21.1.3.{3,5,6}: spec reads only args[0]
// (digits/fractionDigits/precision); trailing operands type_of'd then
// dropped at SSA-emit time.

// --- (n: number).toFixed(digits, ...trailing) ---
const n1: number = 1.5;
console.log(n1.toFixed(2, 99));
console.log(n1.toFixed(0, 1, 2, 3));
console.log(n1.toFixed(4, "junk", true));

const n2: number = 12345.6789;
console.log(n2.toFixed(2, 99));
console.log(n2.toFixed(0, true, false));

// --- (n: number).toExponential(precision, ...trailing) ---
const n3: number = 123.456;
console.log(n3.toExponential(2, 99));
console.log(n3.toExponential(4, 1, 2, 3));

// --- (n: number).toPrecision(precision, ...trailing) ---
console.log(n3.toPrecision(2, 99));
console.log(n3.toPrecision(5, "junk", true));

// --- int receiver path ---
const i1: number = 42;
console.log(i1.toFixed(2, 99));
console.log(i1.toExponential(3, "junk"));
console.log(i1.toPrecision(4, true));
