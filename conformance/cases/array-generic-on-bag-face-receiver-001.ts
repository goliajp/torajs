// §23.1.3 generics over receivers whose whole own face is their expando
// bag (Date / RegExp / Map / Set / Promise / ArrayBuffer / WeakRef):
// `length` and the indices are read the way any name on them is.
function cb(this: any, val: any, idx: any, obj: any) { return val > 0; }
var d: any = new Date(0); d.length = 2; d[0] = 1; d[1] = 5;
var r: any = new RegExp("x"); r.length = 1; r[0] = 3;
var m: any = new Map(); m.length = 3; m[0] = 1; m[2] = 2;
var s: any = new Set(); s.length = 2; s[0] = 4; s[1] = 0;
var p: any = Promise.resolve(1); p.length = 2; p[0] = "a"; p[1] = "b";
var ab: any = new ArrayBuffer(4); ab.length = 1; ab[0] = 7;
var w: any = new WeakRef({}); w.length = 2; w[0] = 8; w[1] = 9;
console.log(Array.prototype.every.call(d, cb), Array.prototype.every.call(r, cb), Array.prototype.every.call(s, cb));
console.log(Array.prototype.some.call(m, cb), Array.prototype.map.call(d, function(v: any) { return v * 2; }));
console.log(Array.prototype.filter.call(m, cb), Array.prototype.indexOf.call(d, 5), Array.prototype.lastIndexOf.call(r, 3));
console.log(Array.prototype.reduce.call(d, function(a: any, b: any) { return a + b; }), Array.prototype.reduceRight.call(p, function(a: any, b: any) { return a + b; }));
console.log(Array.prototype.join.call(p, "-"), Array.prototype.includes.call(ab, 7), Array.prototype.slice.call(w));
var seen: any[] = [];
Array.prototype.forEach.call(r, function(this: any, v: any, i: any, o: any) { seen.push([v, i, o instanceof RegExp]); });
console.log(JSON.stringify(seen));
var empty: any = new Date(0);
console.log(Array.prototype.every.call(empty, cb), Array.prototype.indexOf.call(empty, 1), Array.prototype.some.call(new Map(), cb));
var withGetter: any = new Date(0);
Object.defineProperty(withGetter, "length", { get: function() { console.log("length getter"); return 1; } });
withGetter[0] = 2;
console.log(Array.prototype.find.call(withGetter, cb));
