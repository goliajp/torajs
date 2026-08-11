// rotation 364 — argv-face callbacks on flatMap: the callback reads
// `arguments` values and rides the boxed variadic pack with the full
// §23.1.3.13 «kValue, k, O» triple; the declared ret still drives the
// scalar / inner-walk dst split.
console.log([1, 2].flatMap(function () { return [arguments[0], arguments[1]]; }));
console.log([3, 4].flatMap(function () { return arguments[0] * 2; }));
console.log(["x"].flatMap(function () { return arguments[2].length; }));
// binding form
const f = function () { return [arguments[1], arguments[0]]; };
console.log([7, 8].flatMap(f));
