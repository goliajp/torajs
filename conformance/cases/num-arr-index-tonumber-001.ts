// ES §21.1.3.{3,5,6} step 1 and §23.1.3.39 step 2 are both
// ToIntegerOrInfinity — the operand is coerced, not shape-checked.
console.log((1.5).toFixed("1"));
console.log(Number.NaN.toFixed("1.1"));
console.log(Number.NaN.toFixed("0.9"));
console.log((1.005).toFixed(1.9));
console.log((123.456).toFixed(true as any));
console.log((123.456).toFixed(null as any));
console.log((255).toExponential("2"));
console.log((255).toPrecision("4"));
try { (1).toFixed("101"); } catch (e) { console.log("range", e instanceof RangeError); }

const xs = [1, 2, 3];
console.log(xs.with("1", 9));
console.log(xs.with("-1", 9));
console.log(xs.with(1.7, 9));
console.log(xs.with(true as any, 9));
try { xs.with("9", 0); } catch (e) { console.log("oob", e instanceof RangeError); }

// The operand still evaluates exactly once.
let hits = 0;
const once = { valueOf() { hits++; return 1; } };
console.log((2.5).toFixed(once as any), hits);
