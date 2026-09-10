// %TypedArray%.from / .of spelled on the typed constructor ident (§23.2.2.1-2):
// the lowering boxes the interned ctor cell and rides the any-lane kernels, so a
// this-reading mapfn gets thisArg, and every source shape the any spelling takes works here.
var r = Uint8Array.from([1, 2], function (x: number) { return x * 2; });
console.log(r.length, r[1]);
var q = Uint8Array.from([1, 2], function (this: any, x: number) { return x + this.k; }, { k: 10 });
console.log(q[0], q[1]);
var s = Int16Array.from(new Set([3, 4]));
console.log(s.length, s[0]);
var o = Uint8Array.of(5, 6, 300);
console.log(o.length, o[1], o[2]);
var f = Float64Array.from({ length: 2, 0: "1.5", 1: 2 });
console.log(f.length, f[0]);
var b = BigUint64Array.from([1n, 2n], function (x: bigint, i: number) { return x + BigInt(i); });
console.log(b.length, b[1]);
var c = Uint8ClampedArray.of(-1, 1000, 2.5);
console.log(c[0], c[1], c[2]);
var seen: any[] = [];
Int8Array.from("ab", function (this: any, ch: string, i: number) { seen.push(this, ch, i); return 0; });
console.log(seen.length, seen[0] === undefined, seen[1], seen[5]);
try { Uint8Array.from([1], 5); } catch (err) { console.log((err as any).constructor.name); }
try { BigInt64Array.of(1); } catch (err) { console.log((err as any).constructor.name); }
