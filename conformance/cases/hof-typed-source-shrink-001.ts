// §23.1.3: length is read once, then HasProperty is asked before
// every visit. A callback that shortens its own typed source must not
// be called on the cut-off indices (and must not read their slots).
var calls = 0;
function f(v: number, i: number, o: any) { nums.length = 3; calls++; }
var nums = [1, 2, 3, 4, 5];
nums.forEach(f);
console.log(calls);

var e = [11, 12, 13, 14];
console.log(e.every(function (v) { e.length = 2; return v > 10; }));
var s = [1, 2, 3, 4];
console.log(s.some(function (v) { s.length = 2; return v === 4; }));

var r = [1, 2, 3, 4];
console.log(r.reduce(function (a, v) { r.length = 2; return a + v; }, 0));
var rr = [1, 2, 3, 4];
console.log(rr.reduceRight(function (a, v) { rr.length = 2; return a + v; }, 0));

var fl = [1, 2, 3, 4, 6];
console.log(fl.filter(function () { fl.length = 2; return true; }).length);

// A heap-element source: the cut-off slots were released by the shrink.
var words = ["a".repeat(2), "b".repeat(2), "c".repeat(2)];
var seen: string[] = [];
words.forEach(function (w: string) { words.length = 1; seen.push(w); });
console.log(seen.join(","));

// Shortened through the callback's own `O` parameter.
var viaO = [7, 8, 9];
var n = 0;
viaO.forEach(function (v: number, i: number, o) { o.pop(); n++; });
console.log(n, viaO.length);

// Another array shrinks; this source is untouched and visits all.
var other = [1, 2, 3];
var mine = [4, 5, 6];
var m = 0;
mine.forEach(function () { other.pop(); m++; });
console.log(m, other.length);
