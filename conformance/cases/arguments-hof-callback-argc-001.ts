// HOF inline-loop lanes state the spec argument count in the S1
// hidden argc: map/filter/forEach/predicate/flatMap pass
// «kValue, k, O» = 3, reduce/reduceRight «acc, kValue, k, O» = 4,
// Array.from's mapFn «kValue, k» = 2, Map/Set forEach 3, sort's
// comparator 2 — regardless of how many slots the callback declares
// (the physical arg list stays a transport optimization).
// Pre-knife the argc mirrored the physical list: arguments.length
// answered 1 inside a zero-param map callback where the spec observes 3.
console.log([10, 20].map(function () { return arguments.length; })[0]);
console.log([10, 20].filter(function () { return arguments.length === 3; }).length);
[10].forEach(function () { console.log(arguments.length); });
console.log([10, 20].reduce(function () { return arguments.length; }, 0));
console.log([10, 20].reduceRight(function () { return arguments.length; }, 0));
console.log([10, 20].find(function () { return arguments.length === 3; }));
console.log([10, 20].some(function () { return arguments.length === 3; }));
console.log([10, 20].every(function () { return arguments.length === 3; }));
console.log([10, 20].findIndex(function () { return arguments.length === 3; }));
console.log([10, 20].flatMap(function () { return [arguments.length]; })[0]);
console.log(Array.from([10, 20], function () { return arguments.length; })[0]);
console.log([3, 1, 2].sort(function (a: number, b: number) { return arguments.length - 2 + a - b; }).join(","));
const m = new Map<string, number>([["a", 1]]);
m.forEach(function () { console.log(arguments.length); });
const s = new Set<number>([7]);
s.forEach(function () { console.log(arguments.length); });
console.log([10, 20].map(function (x: number) { return arguments.length + x; })[0]);
