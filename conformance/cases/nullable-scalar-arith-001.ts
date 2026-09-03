// A binding the uninit-let splice could not resolve really may hold
// undefined, and its slot is a NaN-boxed `any` because `number` and
// `boolean` have no bit pattern to spare for it. §13.15.3 and
// §13.7-§13.12 have an answer for every operator on that value —
// ToNumeric(undefined) is NaN — so the any-lane kernels can run it.
// Refusing to compile the file was never one of the answers.
let x: number | undefined;

console.log(x + 1);
console.log(x - 1);
console.log(x * 2);
console.log(x / 2);
console.log(x % 2);
console.log(x ** 2);

// §13.12 is ToInt32 both sides, and ToInt32(NaN) is 0.
console.log(x & 1, x | 1, x ^ 1, x << 1, x >> 1, x >>> 1);

// §7.2.14 with a NaN operand is false in every direction.
console.log(x < 1, x > 1, x <= 1, x >= 1);

// §13.15.3 with a String side concatenates, and ToString(undefined)
// is "undefined".
console.log(x + "s");
console.log("s" + x);

// Unary rides the same lane: §7.1.4 ToNumber, and ToInt32 for `~`.
console.log(-x, +x, ~x, !x);

// Written to, the same binding in the same slot answers as the
// number it now holds.
x = 10;
console.log(x + 1, x * 2, x < 20, -x, ~x);

// A boolean is the other scalar with no spare pattern.
let b: boolean | undefined;
console.log(b + 1, b & 1, !b);
b = true;
console.log(b + 1, b & 1, !b);

// The shape that reaches this without anyone writing the union: a
// read before the write makes the splice decline, so the annotation
// stays `number | undefined`.
let seen: number;
console.log(seen + 1);
seen = 4;
console.log(seen + 1);

// Guard rail for the sibling commit: a binding the splice DID
// resolve keeps its scalar slot, so this stays plain f64 arithmetic
// rather than joining the boxed lane above.
let sum: number;
sum = 0;
for (let i = 0; i < 5; i++) {
  sum += i;
}
console.log(sum, sum + 1, -sum);
