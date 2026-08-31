// Rotation 543 — two more lanes that read what they were handed and
// never gave it back, found by the same bound-vs-temp control that
// found the Object.values / Object.entries pair.
//
// Every Date kernel READS [[DateValue]] off the receiver; none
// consumes it, and `ssa_lower_call_date_methods` had no release site
// at all. `new Date(0).getTime()` returning a NUMBER still leaked,
// which is what ruled out "the callee leaks" — the stranded reference
// is the receiver's, not the result's.
//
// JSON.stringify's step-12 sentinel arm released its argument temp;
// the two walks that actually serialize did not. And slot 1 leaked
// separately from slot 0: a per-call arrow replacer mints a fresh
// closure env every iteration, and the release has to happen BEFORE
// the any box, because `expr_minted_closure` only recognises the
// `Type::Closure` operand.
//
// 200k churn, AOT product RSS, 1.51 MB flat baseline:
//   new Date(0).getTime()               14.37 MB -> 1.62 MB
//   new Date(0).toISOString()           14.35 MB -> 1.88 MB
//   new Date(0).toString()              14.53 MB -> 2.03 MB
//   JSON.stringify({a: 1})              14.57 MB -> 1.82 MB
//   JSON.stringify([1, 2])              27.20 MB -> 1.61 MB
//   JSON.stringify(o, (k, v) => v)      14.78 MB -> 1.93 MB
//
// Bound controls were flat before and after: `d.getTime()` 1.52 MB,
// `JSON.stringify(o)` 1.72 MB, `JSON.stringify(o, rep)` 1.93 MB.
console.log(new Date(0).toISOString(), new Date(0).getTime());
console.log(new Date(0).toString());

const d = new Date(86400000);
console.log(d.getTime(), d.toISOString(), d.getUTCFullYear());
console.log(d.valueOf(), d.getTime(), d.valueOf());

console.log(JSON.stringify({ a: 1, b: [1, 2], c: "x" }));
console.log(JSON.stringify([1, { a: 2 }]));
console.log(JSON.stringify({ a: 1 }, null, 2));
console.log(JSON.stringify({ a: 1 }, null, " ".repeat(3)));
console.log(JSON.stringify({ a: 1, b: 2 }, (k: string, v: any): any => v));
console.log(JSON.stringify(undefined, (k: string, v: any): any => 42));

const o = { a: 1 };
const rep = (k: string, v: any): any => v;
console.log(JSON.stringify(o, rep), JSON.stringify(o, rep), o.a);
console.log(JSON.stringify(o), o.a);
