// An out-of-range `number[]` read answers the undefined-NaN sentinel,
// and every consumer that must say "undefined" rather than "NaN" asks
// one predicate whether the value it holds may be one. The predicate
// looked through an `as` cast and nothing else, so the sentinel went
// silent the moment it passed through any other value-transparent
// wrapper — a ternary, a sequence, an assignment used as an
// expression. Silent is the whole difficulty: an unrecognised
// sentinel is simply a NaN.
let xs: number[] = [1, 2, 3];
console.log(xs[9]);
console.log(true ? xs[9] : xs[0]);
console.log(false ? xs[0] : xs[9]);
console.log((0, xs[9]));
let a: number = 0;
console.log((a = xs[9]));
console.log((true ? xs[9] : 0) as number);

// The wrappers compose, and a let-init reaches the same predicate.
let b: number = true ? (0, xs[9]) : 0;
console.log(b);

// Consumers other than print: typeof and strict equality.
console.log(typeof (true ? xs[9] : 0));
console.log((0, xs[9]) === undefined);
console.log((true ? xs[9] : 0) === undefined);

// A nullish yields its left arm only when that arm is neither null
// nor undefined, so a sentinel there can never come out; the right
// arm can.
console.log(xs[0] ?? xs[9]);
console.log(typeof (xs[9] ?? xs[1]));

// In-range reads through the same wrappers stay numbers.
console.log(true ? xs[1] : 0);
console.log(typeof (0, xs[2]));
