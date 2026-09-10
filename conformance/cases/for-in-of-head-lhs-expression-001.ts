// §14.7.5 — a for-in / for-of head may be any LeftHandSideExpression,
// not only a name: a member, an index, the parenthesized cover of a
// name, `async.x`. The reference is evaluated and assigned per
// iteration (test262 statements/for-{in,of}/head-lhs-*).
var log: string[] = [];
var x: any = {};
for (x.y of [23, 24]) log.push("y=" + x.y);
var arr: any[] = [0, 0];
var i = 0;
for (arr[i++] of ["a", "b"]) ;
var z: any;
for ((z) of [7]) ;
var async: any = { x: 0 };
for (async.x of [1]) ;
for (x.k in { attr: null, other: 1 }) log.push("k=" + x.k);
var w: any;
for ((w) in { only: 1 }) ;
function holder() { log.push("holder"); return x; }
for (holder().h of [1, 2]) ;
console.log(log.join(","), arr.join("|"), z, async.x, w, x.h);
