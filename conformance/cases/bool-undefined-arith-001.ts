// §13.15.3 ApplyStringOrNumericBinaryOperator over a Boolean and
// Undefined pair: ToNumber(true) is 1, ToNumber(undefined) is NaN, and
// every arithmetic operator answers NaN. The undefined lane landed
// first and put its NaN sentinel on the f64 path; a Boolean beside it
// had no arm in the f64 sink at all, so `true + undefined` was a
// compile-time refusal while `undefined + 1` already answered NaN.
const t = true;
const f = false;
const u = undefined;

console.log(t + u);
console.log(u + t);
console.log(f + u);
console.log(t - u);
console.log(t * u);
console.log(t / u);
console.log(t % u);
console.log(t ** u);

// a computed (non-literal) boolean takes the same path
const c = 1 < 2;
console.log(c + u);
console.log((3 > 4) - u);

// the pairs that already worked keep their answers
console.log(t + 1);
console.log(f * 2);
console.log(u + 1);
console.log(String(t + u));
console.log(isNaN(t + u));
