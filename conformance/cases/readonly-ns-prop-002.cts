// Sloppy goal — the same writes silently fail; the assignment
// expression still yields its rhs (§13.15.2).
Number.NaN = 1;
console.log(Number.NaN === 1, Number.NaN !== Number.NaN);
Math.PI = 3;
console.log(Math.PI);
var got = (Number.EPSILON = 7);
console.log(got, Number.EPSILON === 7);
