// console.log prints Symbol-keyed own enumerable properties as
// `[Symbol(desc)]: value`, in bun's order: pure insertion order with
// the symbols in place when the object has no array-index key, and
// index keys → string keys → symbols once it has one. Covers plain
// objects, class instances with Symbol expandos, nesting, and the
// line-width accounting of a Symbol key (rotation 560-02).
const s = Symbol("s"), t = Symbol("t");
const o: any = { a: 1, [s]: 2, [Symbol()]: 3, [Symbol.iterator]: 4 };
console.log(o);
console.log({ b: 1, [s]: 2, a: 4, [t]: 5 } as any);
console.log({ b: 1, [s]: 2, 1: 3, a: 4, [t]: 5, 0: 6 } as any);
class A { x = 1; }
const a: any = new A(); a[s] = 5; a.y = 6; a[1] = 7; a[t] = 8; a.z = 9;
console.log(a);
class B { x = 1; }
const b: any = new B(); b[s] = 5; b.y = 6;
console.log(b);
const c: any = {}; c[s] = 5; c.y = 6;
console.log(c);
console.log({ [s]: { [s]: 1 } });
console.log([{ [s]: 1 }]);
// Width accounting: the same 30-element array under a short string
// key, a Symbol key, and a long-description Symbol key.
const nums = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30];
const p: any = {}; p[s] = nums; p.k = nums;
console.log(p);
const q: any = {}; q[Symbol("a-longer-description-here-xx")] = nums;
console.log(q);
const r: any = new B(); r[Symbol("a-longer-description-here-xx")] = nums;
console.log(r);
// Array expandos share the key writer: quoted non-identifier keys,
// Symbol keys, nested values.
const ar: any = [1, 2]; ar[s] = 3; ar.k = 4; ar["a-b"] = 5; ar[Symbol()] = 6;
console.log(ar);
const br: any = [1]; br[s] = { x: [1, 2] };
console.log(br);
console.log([ar]);
