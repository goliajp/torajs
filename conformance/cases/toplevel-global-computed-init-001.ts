// The other half of the same gap: a top-level binding whose init is
// an ordinary computation could not become a data global, so no named
// fn body could see it. Every operator here answers a type the spec
// fixes — a boolean for the comparisons and `!`, an integer for the
// bitwise ones and shifts, a number for the rest — and the WIDTH of
// the numeric ones is left to `num_width`, which keys every top-level
// let as a global whether or not it promotes.
const i1: number = 7;
const i2: number = 3;

const d = i1 / i2;
const q = 1 / 4;
const fracAdd = 0.5 + 0.25;
function fractions(): string {
  return d + "|" + q + "|" + fracAdd;
}
console.log(fractions());

const mul = i1 * i2;
const sub = i1 - i2;
const mod = i1 % i2;
const pow = i1 ** 2;
const add = i1 + i2;
function integers(): number {
  return mul + sub + mod + pow + add;
}
console.log(integers(), mul, sub, mod, pow, add, typeof mul);

const bits = i1 & i2;
const shifted = i1 << 2;
const unsigned = -1 >>> 28;
function bitwise(): number {
  return bits + shifted + unsigned;
}
console.log(bitwise(), bits, shifted, unsigned);

const cmp = i1 > i2;
const eq = i1 === i2;
const negated = !cmp;
function flags(): string {
  return cmp + "|" + eq + "|" + negated;
}
console.log(flags(), typeof cmp);

// a write from a named fn body lands in the same slot
let counter = 1 + 1;
function bump(): void {
  counter = counter * 2;
}
bump();
bump();
console.log(counter);

// and the value is an ordinary number everywhere else it is used
const arr: number[] = [10, 20, 30];
const idx = 1 + 1;
function pick(): number {
  return arr[idx];
}
console.log(pick());

// past the integer range, where the slot has to be the wide one
const near: number = 9007199254740993;
const wide = near + 2;
function widened(): number {
  return wide;
}
console.log(widened());

// `&&` / `||` yield an operand rather than a fresh value of a fixed
// type, so they stay out — the read below is the main side's, which
// has always worked
const either = i1 > 0 && i2 > 0;
console.log(either);
