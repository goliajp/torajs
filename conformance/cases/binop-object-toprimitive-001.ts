// Every binary operator sends an object operand through ToPrimitive
// first, so a `valueOf` hook participates. Four operator families had
// the arm for this and four did not, which made the difference between
// `t * 2` (worked) and `t + 1` (compile error) look arbitrary.
//
// The any-lane kernels already carried the whole walk — valueOf then
// toString, either answer, throws propagated — because an `any`-typed
// operand has always used them. Boxing the object side is what routes
// it there, so the object case and the `any` case are one path.

const t = { valueOf() { return 42; } };

// §13.7-§13.9 ToNumeric — these already worked
console.log(t * 2, t - 1, t / 2, t % 5);

// §13.6 — Pow
console.log(t ** 2);

// §13.15.3 with the DEFAULT hint. A valueOf answering a number adds;
// the same operator concatenates when the answer is a string, which is
// why this cannot be typed Number at compile time.
console.log(t + 1, 1 + t, t + t);

// §13.10 ordering — ToPrimitive with the NUMBER hint, then compare.
// One per line: `t < 10, t >= 42` on one line parses as a generic
// instantiation `t<10, t>` followed by an assignment, which is a TS
// grammar ambiguity and nothing to do with this.
console.log(t > 10);
console.log(t < 10);
console.log(t >= 42);
console.log(t <= 41);

// §13.12 bitwise — ToInt32 / ToUint32
console.log(t & 3, t | 1, t ^ 5, t << 1, t >> 1, t >>> 1);

// a valueOf answering a string: the DEFAULT hint takes it, so `+`
// concatenates while the numeric operators coerce it
const s = { valueOf() { return "7"; } };
console.log(s + 1);
console.log(s * 2);

// no valueOf — ToPrimitive falls through to toString
const plain = { a: 1 };
console.log(plain + 1);
const named = { toString() { return "N"; } };
console.log(named + 1);

// a class instance reaches its prototype's hook the same way
class Money {
  constructor(public cents: number) {}
  valueOf(): number {
    return this.cents;
  }
}
const m = new Money(250);
console.log(m + 10, m * 2, m ** 1);
console.log(m > 100);

// the string-concat lane must not move: it is typed String, not Any
console.log("s" + t);
console.log(t + "s");
console.log("x" + named);

// nor may the Bool / Null coercions
console.log(true + false, null + 1, true + 1, null + null, true + null);

// nor the plain numeric lanes
console.log(2 + 3, 7 % 4, 2 ** 10, 6 & 3);
console.log(1 < 2);
