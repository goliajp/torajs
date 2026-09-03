// §13.13 — `a && b` IS one of its two operands: `a` when `a` is
// falsy, `b` otherwise. Every pair of types is therefore legal, and
// what a mismatched pair needs is a name for the join. The checker
// had no general union type to spell `A | B` with, and instead of
// naming the join Any it refused: `flag && count` did not compile,
// with a message about "matching operand types" that no rule of the
// language asks for.

const n = 3;
const s = "hi";
const flag = true;
const count = 7;

// The two orders answer different operands, so both are worth a line.
console.log(n && s, s && n);
console.log(flag && count, count && flag);

// A falsy left operand IS the answer -- not `false`, not coerced.
console.log(0 && s, "" && n, false && count);

// `||` is the mirror: the left operand when it is truthy.
console.log("" || n, 0 || s, n || s, s || n);

// The join is a real value: its type, its length, its use as an
// argument all behave like whichever operand won.
const j = n && s;
console.log(j, typeof j, String(j).length);
console.log(typeof (0 && s), typeof (0 || s), typeof (flag && count));

// An object on one side, a primitive on the other.
const o = { v: 1 };
console.log(o && "yes", o.v || s);

// Chained, so the join of a join is itself an operand.
console.log(n && s && flag, 0 || "" || n);

// The pairs that already compiled still answer exactly what they did.
const a: number | null = null;
console.log(a || n, n || 0, s || "x");
