// Object.is (§20.1.2.9 SameValue) across the internal i64/f64
// representation split — JS Number is one type, so a computed f64
// equals the same-valued integer literal.

const half = 0.5;
const one = half + half;
console.log(Object.is(one, 1), Object.is(1, one), one === 1);

console.log(Object.is(-0, 0), Object.is(0, -0), Object.is(NaN, NaN));

const zf = -0.5 * 0;
console.log(Object.is(zf, 0), Object.is(0, zf));

console.log(Object.is(2.5, 2), Object.is(2, 2.5));
console.log(Object.is(9007199254740990 + 1, 9007199254740991));
