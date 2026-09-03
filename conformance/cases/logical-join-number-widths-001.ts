// A number is ONE type to the checker and TWO widths to the lowering.
// `&&` and `||` reconciled nothing: the join slot took whichever arm
// the lowering saw first, and an f64 operand stored into an i64 slot
// is not a truncation the backend does quietly -- it refuses. So
// `xs.length && xs[0]` -- an i64 length beside an f64 element, and
// about as ordinary a guard as the language has -- was a hard
// compile error, not a wrong answer. The ternary next door has
// settled this at f64 since W3 S8.

const ints = [1, 2, 3];
const fracs = [1.5, 2.5];
const empty: number[] = [];

// The guard idiom, both element widths, both operand orders.
console.log(ints.length && ints[0], ints[0] && ints.length);
console.log(fracs.length && fracs[0], fracs[0] && fracs.length);
console.log(ints.length || ints[0], fracs[0] || fracs.length);

// The falsy operand IS the answer, and it keeps its own value --
// the join widening must not turn a zero-length guard into 0.0 or
// into `false`.
console.log(empty.length && empty[0], empty.length || -1);
console.log(0 && fracs[0], 0 || fracs[0]);

// Plain bindings of each width, both orders, both operators.
const n = 2;
const f = 0.5;
console.log(n && f, f && n, n || f, f || n);
console.log(0 && f, 0 || f, f && 0, f || 0);

// The join is a number like any other: it adds, and it reports its
// own type.
const first = ints.length && ints[0];
const firstF = fracs.length && fracs[0];
console.log(first + 1, firstF + 1, typeof first, typeof firstF);

// Chained, so a join is itself an arm of the next one.
console.log(ints.length && fracs.length && fracs[1]);
console.log(empty.length || fracs.length || 99);
