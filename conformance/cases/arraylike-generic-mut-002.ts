// RFC 20260712-array-generic-receiver chunk 3b-2 — generic splice /
// sort / fill / copyWithin over plain-object receivers: splice
// returns the removed Gets as a fresh array and rides Has-gated gap
// moves; sort stages the present Gets through the kind-aware
// merge-sort kernel and Sets the prefix back (hole tail deletes);
// fill / copyWithin are relative-wrapped Set walks (copyWithin
// direction-aware on overlap).
//
// Acceptance: byte-equal with bun.

var o: any = { 0: 1, 1: 2, 2: 3, 3: 4, length: 4 };
o.splice = Array.prototype.splice;
console.log(o.splice(1, 2, "a"));
console.log(o.length, o[0], o[1], o[2], o[3]);
var g: any = { 0: 1, length: 1 };
g.splice = Array.prototype.splice;
console.log(g.splice(0, 0, "x", "y"));
console.log(g.length, g[0], g[1], g[2]);

var s: any = { 0: 3, 1: 1, 2: 2, length: 3 };
s.sort = Array.prototype.sort;
s.sort();
console.log(s[0], s[1], s[2]);
s.sort((a: any, b: any) => b - a);
console.log(s[0], s[1], s[2]);
var holes: any = { 0: "b", 2: "a", length: 3 };
holes.sort = Array.prototype.sort;
holes.sort();
console.log(holes[0], holes[1], holes[2], 2 in holes);

var f: any = { 0: 1, 1: 2, 2: 3, length: 3 };
f.fill = Array.prototype.fill;
console.log(f.fill("z", 1) === f);
console.log(f[0], f[1], f[2]);

var c: any = { 0: 1, 1: 2, 2: 3, 3: 4, 4: 5, length: 5 };
c.copyWithin = Array.prototype.copyWithin;
c.copyWithin(1, 3);
console.log(c[0], c[1], c[2], c[3], c[4]);
var c2: any = { 0: 1, 1: 2, 2: 3, 3: 4, length: 4 };
c2.copyWithin = Array.prototype.copyWithin;
c2.copyWithin(1, 0, 3);
console.log(c2[0], c2[1], c2[2], c2[3]);
