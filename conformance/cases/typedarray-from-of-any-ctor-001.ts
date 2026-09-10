// %TypedArray%.from / .of through an any-valued constructor (§23.2.2.1 / §23.2.2.2):
// the statics live on %TypedArray%, so a `: any` TA binding, a parameter and an
// `as any` cast all reach the same kernel; mapfn sees thisArg (or undefined).
var source = [42, 43];
var thisArg = { tag: "T" };

function run(f: any) { f(Int8Array); f(Uint16Array); f(Float32Array); }
run(function (C) {
  var results: any[] = [];
  var mapfn = function (this: any, v: number, k: number) { results.push(this); return v + k; };
  var r = C.from(source, mapfn, thisArg);
  console.log(C.name, r.length, r[0], r[1], results.length, results[0] === thisArg, results[1] === thisArg);
  var none: any[] = [];
  C.from(source, function (this: any) { none.push(this); return 1; });
  console.log(none.length, none[0] === undefined);
});

var TA: any = Uint8Array;
var a = TA.from([1, 2, 3]);
console.log(a.length, a[2], a instanceof Uint8Array);
var b = TA.from(new Set([5, 6]), function (x: number, i: number) { return x * 10 + i; });
console.log(b.length, b[0], b[1]);
var c = TA.from({ length: 2, 0: "7", 1: 300 });
console.log(c.length, c[0], c[1]);
var d = TA.of(9, 8, 7);
console.log(d.length, d[0], d[2]);
var e = TA.from("abc", function (ch: string) { return ch.charCodeAt(0); });
console.log(e.length, e[1]);

var order: string[] = [];
var it = { [Symbol.iterator]: function* () { order.push("a"); yield 1; order.push("b"); yield 2; } };
TA.from(it, function (x: number) { order.push("m" + x); return x; });
console.log(order.join(","));

var B: any = BigInt64Array;
var f = B.from([1n, 2n], function (this: any, x: bigint) { return x * 2n; });
console.log(f.length, f[1]);
var g = (Uint8ClampedArray as any).of(300, -5);
console.log(g[0], g[1]);
console.log((Uint8Array as any).from === (Int8Array as any).from, TA.from.name, TA.from.length, TA.of.length);
try { TA.from.call({}, [1]); } catch (err) { console.log((err as any).constructor.name); }
try { B.from([1]); } catch (err) { console.log((err as any).constructor.name); }
try { TA.from([1], 5); } catch (err) { console.log((err as any).constructor.name); }
class Sub extends Uint8Array {}
var S: any = Sub;
var h = S.from([1, 2]);
console.log(h instanceof Sub, h.length, h[1]);
