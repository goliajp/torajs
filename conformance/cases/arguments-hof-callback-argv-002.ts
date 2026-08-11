// Rotation 363 follow-up — the collector's HOF-anon arm keys on the
// METHOD NAME (map/filter/forEach), so a Map/Set receiver's forEach
// admits the same argv-face callbacks; the map/set dispatch loops
// route them through the boxed variadic pack like the array loops
// (pre-fix the positional (value, key, map) landed in the argv
// pointer slot and the loop was a silent no-op — probe x1).
const m = new Map<string, number>([["a", 1]]);
m.forEach(function () { console.log(arguments.length, arguments[1]); });
const s = new Set<number>([7]);
s.forEach(function () { console.log(arguments[0]); });
