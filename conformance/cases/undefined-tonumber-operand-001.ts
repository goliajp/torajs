// §7.1.4 ToNumber has an answer for `undefined` (NaN) in the same
// table that gives `null` one (+0), and the operator families read
// only the null half: `null * 3` ran while `undefined * 3` was a
// compile error — including when the very same value was spelled
// `any`, which ran and answered NaN.
//
// The two halves are the same operand shape at the SSA layer (both
// spell as a null pointer), so admitting the type without splitting
// them apart would have answered 0 — worse than the refusal. The
// split is one substitution before the lane is picked.
const u = undefined;
const n = null;

// arithmetic — ToNumber(undefined) is NaN
console.log(u + 1, u - 1, u * 3, u / 2, u % 2, u ** 2);
console.log(1 + u, 1 - u, 3 * u, 2 / u, 2 % u, 2 ** u);
console.log(u + u, u - n, n - u);

// the literal spelling takes the same route as the binding
console.log(undefined * 3, undefined - 1, 1 + undefined);

// ordering — every comparison against NaN is false
console.log(u < 1, u > 1, u <= 1, u >= 1);

// the bitwise six — ToInt32(NaN) is 0, exactly ToInt32(+0)
console.log(u | 0, u & 1, u ^ 1, u << 1, u >> 1, u >>> 1);

// `+` with a String side keeps §13.15.3's concat branch
console.log("x" + u, u + "x");

// loose equality never reaches ToNumber for a nullish side
console.log(u == 0, u == null, u == undefined, n == 0);

// compound assignment desugars to the same binop, and the answer has
// to survive the store: a number slot the analysis picked as an int
// cannot hold NaN.
let a = 10;
a -= u;
console.log(a);
let b = 10;
b *= u;
console.log(b);
let c = 10.5;
c += u;
console.log(c);
let d = 10;
d |= u;
console.log(d);

// `**` never had the coercible-operand arm its four arithmetic
// siblings have — §13.6 step 1 is the same ToNumeric call — so
// `null ** 2` and even `true ** 2` were compile errors.
console.log(n ** 2, true ** 2, false ** 2, 2 ** n);

// the null half, unchanged
console.log(n + 1, n - 1, n * 3, n | 0, n < 1, n > 1);

// through a typed binding and a function result
let e: undefined = undefined;
console.log(e * 3, e + 1, e | 0, e < 1);
function g(): undefined { return undefined; }
console.log(g() * 3, g() + 1, g() | 0);

// and the `any` spelling, which has answered correctly all along
const q: any = undefined;
console.log(+q, -q, q * 3, q - 1, q + 1, q | 0, "x" + q, String(q), Number(q));
