// §13.5.3 — `typeof` on a name that resolves to an ACCESSOR property of
// the global object runs the getter (GetValue) and answers the result's
// type; the kernel used to box the AccessorPair cell and answer "object".
// oracle: node script goal.
var calls = 0;
Object.defineProperty(this, "acc", { get: function () { calls++; return 41 + 1; }, configurable: true });
console.log(typeof acc, calls);
Object.defineProperty(this, "accStr", { get: function () { return "s" + "t"; }, configurable: true });
console.log(typeof accStr, typeof nope);
function f() { return typeof acc; }
console.log(f(), calls);
Object.defineProperty(this, "boom", { get: function () { throw new RangeError("g"); }, configurable: true });
try { console.log(typeof boom); } catch (e) { console.log(e.constructor.name); }
function g() { return typeof boom; }
try { g(); } catch (e) { console.log("inner", e.constructor.name); }
