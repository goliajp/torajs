// RFC 20260808-construct-channel B6 刀 3 — the mapFn escape shapes
// route the any-tier kernel (shared checker/lowering predicate):
// struct source elements really read, «kValue, k» exact call shape,
// an explicit thisArg binds, and a non-callable mapfn is the
// runtime step-2 TypeError. The Str/Arr/Set + Function fast lane
// keeps the typed devirt loop (last probe).
var list = { '0': 41, '1': 42, '2': 43, length: 3 };
const a = Array.from(list, function (v: any, k: any) { return v * 2 + k; });
console.log(a.length, a[0], a[1], a[2]);
const b = Array.from([10, 20], function (v: any, k: any) { return v + k; }, { t: 1 });
console.log(b.length, b[0], b[1]);
try { Array.from([1], 5 as any); } catch (e) { console.log("caught-notfn"); }
try { Array.from([1], "s" as any); } catch (e) { console.log("caught-str"); }
const c = Array.from([3, 4], (v: any) => v * 10);
console.log(c.length, c[0], c[1]);
