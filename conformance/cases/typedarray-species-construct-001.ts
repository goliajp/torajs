const ta = new Uint8Array([1, 2, 3, 4]) as any;
ta.constructor = { [Symbol.species]: function (n: number) { return new Int16Array(n + 2); } };
const m = ta.filter((x: number) => x > 1);
console.log(Object.prototype.toString.call(m), m.length, m[0], m[1], m[2]);
const s = ta.slice(1, 3);
console.log(Object.prototype.toString.call(s), s.length, s[0], s[1]);
const mp = ta.map((x: number) => x * 2);
console.log(Object.prototype.toString.call(mp), mp.length, mp[0], mp[3]);
// subarray hands the species « buffer, byteOffset, length ».
const ta2 = new Uint8Array([9, 8, 7, 6]) as any;
ta2.constructor = {
  [Symbol.species]: function (b: ArrayBuffer, o: number, l: number) {
    console.log("subargs", b instanceof ArrayBuffer, o, l);
    return new Uint8Array(b, o, l);
  },
};
const sub = ta2.subarray(1, 3);
console.log(sub.length, sub[0], sub[1], sub.buffer === ta2.buffer);
// Too-small product → TypeError.
const ta3 = new Uint8Array([1, 2, 3]) as any;
ta3.constructor = { [Symbol.species]: function (n: number) { return new Uint8Array(0); } };
try { ta3.slice(0); } catch (e) { console.log("small", (e as Error).constructor.name); }
// Non-TypedArray product → TypeError.
const ta4 = new Uint8Array([1]) as any;
ta4.constructor = { [Symbol.species]: function () { return {}; } };
try { ta4.map((x: number) => x); } catch (e) { console.log("notta", (e as Error).constructor.name); }
// undefined / null species → default product.
const ta5 = new Uint8Array([5, 6]) as any;
ta5.constructor = { [Symbol.species]: undefined };
const d5 = ta5.slice(0);
console.log(Object.prototype.toString.call(d5), d5[0]);
// Throwing species ctor propagates.
const ta6 = new Uint8Array([1]) as any;
ta6.constructor = { [Symbol.species]: function () { throw new RangeError("boom"); } };
try { ta6.slice(0); } catch (e) { console.log("threw", (e as Error).constructor.name); }
// Primitive constructor still throws (the old guard face).
const ta7 = new Uint8Array([1]) as any;
ta7.constructor = 5;
try { ta7.slice(0); } catch (e) { console.log("prim", (e as Error).constructor.name); }
console.log("end");
