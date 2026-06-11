// W3 S8 (rfc 20260611-ann-width-unification §5.3) — unary negation
// on a runtime int must preserve IEEE -0: `-x` mints -0 iff x == 0.

// s8 main reproduction: 1 / -0 must be -Infinity
function f(a: number): number {
  return -a;
}
console.log(1 / f(0)); // -Infinity
console.log(1 / f(5)); // -0.2
console.log(f(-3)); // 3
console.log(Object.is(f(0), -0)); // true
console.log(Object.is(f(0), 0)); // false

// literal negation stays the const fold
console.log(-8); // -8
console.log(1 / -0); // -Infinity

// negation result feeding int contexts (truncation-recovery shapes)
let x = 7;
console.log(-x); // -7
console.log(-x | 0); // -7  (ToInt32 sink)

// accumulator over negations
let s = 0;
for (let i = 1; i <= 3; i++) {
  s += -i;
}
console.log(s); // -6

// double negation round-trips
let y = 4;
console.log(-(-y)); // 4
console.log(1 / -(-y)); // 0.25
