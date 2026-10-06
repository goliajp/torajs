// A typed array stored into a field whose declared type is `any`
// (interface, inline object type, class field, nested and cyclic
// aliases) reaches the any world through that field.
interface B {
  v: any;
  n: number;
}
const c1: number[] = [3, 1, 2];
const b: B = { v: c1, n: 1 };
b.v.push("i");
console.log(c1.length, c1[3], b.n);

const c2: number[] = [3, 1, 2];
const o: { v: any } = { v: null };
o.v = c2;
o.v.push({ k: 1 });
console.log(c2.length, c2[1]);
console.log(c2);

class K {
  v: any = null;
}
const c3: number[] = [3, 1, 2];
const k = new K();
k.v = c3;
k.v[0] = "k";
console.log(c3.length, c3[0], c3[1] + c3[2]);

type W = { inner: { v: any } };
const c4: number[] = [3, 1, 2];
const w: W = { inner: { v: c4 } };
w.inner.v.unshift("w");
console.log(c4.length, c4[0], c4[1]);

type N = { v: any; next: N | null };
const c5: number[] = [3, 1, 2];
const n: N = { v: null, next: { v: c5, next: null } };
n.next!.v.push("cy");
console.log(c5.length, c5[3]);

function put(x: B): void {
  x.v.push(undefined);
}
const c6: number[] = [8];
put({ v: c6, n: 2 });
console.log(c6.length, c6[1]);
