// rotation 364 — argv-face comparator on sort/toSorted: the callback
// reads `arguments` values and rides the boxed variadic pack with the
// §23.1.3.30.2 «x, y» pair; an Any-typed comparator return coerces
// through ToNumber before the `> 0` test.
console.log([3, 1, 2].sort(function () { return arguments[0] - arguments[1]; }));
console.log([1, 2, 3].sort(function () { return arguments[1] - arguments[0]; }));
// Any-ret body (raw arguments[0] passthrough would be NaN-box bits)
console.log([2, 1].sort(function () { return arguments[0] < arguments[1] ? -1 : 1; }));
// binding form + toSorted (source untouched)
const c = function () { return arguments[0] - arguments[1]; };
const xs = [9, 7, 8];
console.log(xs.toSorted(c));
console.log(xs);
// string elements through the comparator
console.log(["b", "c", "a"].sort(function () { return arguments[0] < arguments[1] ? -1 : 1; }));
