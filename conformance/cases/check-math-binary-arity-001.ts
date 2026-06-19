// S205 — Math binary methods (pow / atan2 / imul) accept fewer-arg
// forms per ES default-undefined.
// Spec refs:
//   §21.3.2.26 Math.pow(base, exponent)   — ToNumber both args
//   §21.3.2.5  Math.atan2(y, x)           — ToNumber both args
//   §21.3.2.19 Math.imul(x, y)            — ToUint32 both args
//
// ToNumber(undefined) = NaN, and NaN propagates through pow / atan2
// → both 0-arg and 1-arg call the helper with at least one NaN
// operand and return NaN. ToUint32(undefined) = +0, and imul(0, …)
// short-circuits to 0.

// pow / atan2 — NaN family.
console.log('pow-0arg:', Math.pow());
console.log('pow-1arg:', Math.pow(2));
console.log('atan2-0arg:', Math.atan2());
console.log('atan2-1arg:', Math.atan2(1));

// imul — uint32 zero family.
console.log('imul-0arg:', Math.imul());
console.log('imul-1arg:', Math.imul(2));

// Number.isNaN-confirmed (NaN !== NaN footgun guard).
console.log('isNaN-pow0:', Number.isNaN(Math.pow()));
console.log('isNaN-pow1:', Number.isNaN(Math.pow(3)));
console.log('isNaN-atan2_0:', Number.isNaN(Math.atan2()));
console.log('isNaN-atan2_1:', Number.isNaN(Math.atan2(2)));

// 2-arg regression — make sure the helper Call path still works.
console.log('pow(2,10):', Math.pow(2, 10));
console.log('pow(2.5,3):', Math.pow(2.5, 3));
console.log('atan2(1,1):', Math.atan2(1, 1));
console.log('imul(5,7):', Math.imul(5, 7));
console.log('imul(-1,-1):', Math.imul(-1, -1));
