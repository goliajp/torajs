// rotation 364 — argv-face mapFn on Array.from: the callback reads
// `arguments` values and rides the boxed variadic pack with the
// §23.1.2.1 «kValue, k» pair (anonymous and binding forms).
console.log(Array.from([5, 6], function () { return arguments[0] * 10 + arguments[1]; }));
console.log(Array.from([5, 6], function () { return arguments.length; }));
// binding form
const m = function () { return arguments[1] * 100 + arguments[0]; };
console.log(Array.from([7, 8], m));
