// Casts to a struct type whose fields are `any`, any fields of generic
// and deeply nested types, and a cast chain that ends back in a typed
// array type.
interface B {
  v: any;
}
const o1 = { v: [1, 2] };
(o1 as B).v.push("b");
console.log(o1.v.length, o1.v[2]);

const o2 = { v: [1, 2] };
(o2 as { v: any }).v.unshift(null);
console.log(o2.v.length, o2.v[0], o2.v[1]);

interface Box<T> {
  v: any;
  t: T;
}
const c3: number[] = [1, 2];
const b3: Box<number> = { v: c3, t: 1 };
b3.v[0] = "x";
console.log(c3.length, c3[0], b3.t);

type T1 = { a: T2 };
type T2 = { a: T3 };
type T3 = { a: T4 };
type T4 = { a: T5 };
type T5 = { a: T6 };
type T6 = { a: T7 };
type T7 = { a: T8 };
type T8 = { a: T9 };
type T9 = { v: any };
const c4: number[] = [1, 2];
const t4: T1 = { a: { a: { a: { a: { a: { a: { a: { a: { v: c4 } } } } } } } } };
t4.a.a.a.a.a.a.a.a.v.push("deep");
console.log(c4.length, c4[2]);

const xs: number[] = [1, 2];
const ys = xs as unknown as number[];
ys.push(3.5);
let s = 0;
for (const v of xs) s += v;
console.log(xs, ys.length, s);
