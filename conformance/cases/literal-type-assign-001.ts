// Literal types (TS §3.2.10): a declared literal type, a union of
// literals, and a `const` holding a literal. Each value is a value of
// the base type: it reads, compares, concatenates and does arithmetic
// as that type.

type Mode = "dev" | "prod";
type Bit = 0 | 1;
type Yes = true;
type One = "one";

let mode: Mode = "dev";
console.log(mode, mode.length, mode.toUpperCase());
mode = "prod";
console.log(mode === "prod", mode + "!");

let bit: Bit = 0;
let sum = 0;
for (let i = 0; i < 5; i++) {
  bit = bit === 0 ? 1 : 0;
  sum += bit * 10 + i;
}
console.log(bit, sum, bit + 0.5);

const yes: Yes = true;
console.log(yes, !yes, yes && "and");

const one: One = "one";
console.log(one.charAt(0), [one, one].join("-"));

const k = "key";
const n = 3;
const flag = false;
console.log(k.length, n * 2, flag || "or", typeof k, typeof n);

let widened = "start";
widened = widened + "-more";
console.log(widened);

const s: string = one;
const m: number = bit;
console.log(s, m);
