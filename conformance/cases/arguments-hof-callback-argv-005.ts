// rotation 364 — argv-face callbacks on the predicate family:
// find/findLast/findIndex/findLastIndex/some/every callbacks reading
// `arguments` values ride the boxed variadic pack with the full
// §23.1.3.{5-10,30} «kValue, k, O» triple (anonymous and binding forms).
console.log([10, 20, 30].find(function () { return arguments[0] === 20; }));
console.log([10, 20, 30].findIndex(function () { return arguments[1] === 2; }));
console.log([10, 20, 30].some(function () { return arguments[0] > 25; }));
console.log([10, 20, 30].every(function () { return arguments.length === 3; }));
console.log([10, 20, 30].findLast(function () { return arguments[0] < 25; }));
console.log([10, 20, 30].findLastIndex(function () { return arguments[0] === 10; }));
// binding form — the safe-chain walk admits the callback-slot use
const p = function () { return arguments[0] >= 20 && arguments[2].length === 3; };
console.log([10, 20, 30].filter(p).length);
console.log([10, 20, 30].findIndex(p));
// source-array read through arguments[2]
console.log(["a", "b"].some(function () { return arguments[2][1] === "b"; }));
