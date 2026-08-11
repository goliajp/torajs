// Rotation 363 knife 3 — a BINDING handed to the HOF callback slot
// (map/filter/forEach) joins the argv face too: the slot is
// boxed-only consumption (every downgraded channel routes argv-face
// callees through the boxed variadic pack), so the safe-chain walk
// no longer kills the chain there. One binding may serve several
// slots (`two` below) — each use is legal independently.
const f = function () { return arguments[0]; };
const r = [10, 20].map(f);
console.log(r[0], r[1]);
const g = function () { return arguments[1]; };
console.log([9].map(g)[0]);
const p = function () { return arguments[0] > 1; };
console.log([1, 2, 3].filter(p).join(","));
const e = function () { console.log(arguments[1]); };
["x"].forEach(e);
const two = function () { return arguments[0] + arguments[1]; };
console.log([10].map(two)[0], [20, 30].map(two).join(","));
