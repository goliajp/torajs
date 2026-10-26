// An object of an inferred shape (an unannotated literal, a class
// field with no annotation, a literal an unannotated function returns)
// handed to a parameter of a declared object type that is stored an
// `any` value: the store writes the object's own slot (§10.1.9), and a
// read through the original binding sees the value stored.
class C {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}

function st(o: { c: C }, v: any): void {
  o.c = v;
}

const raw: any = JSON.parse('{"x": "s", "y": 7}');

const o1 = { c: new C(1) };
st(o1, raw);
console.log(o1.c === raw, o1.c.x);

class K {
  c = new C(2);
}
const k = new K();
st(k, raw);
console.log(k.c === raw, k.c.x);

function mk() {
  return { c: new C(3) };
}
const o3 = mk();
st(o3, raw);
console.log(o3.c === raw, o3.c.x);

let o4 = { c: new C(4), n: 1 };
st(o4, raw);
console.log(o4.c === raw, o4.c.x, o4.n);

const o5 = { inner: { c: new C(5) } };
st(o5.inner, raw);
console.log(o5.inner.c === raw, o5.inner.c.x);

const arr = [{ c: new C(6) }];
st(arr[0], raw);
console.log(arr[0].c === raw, arr[0].c.x);

function rd(o: { c: C }) {
  return o.c.x;
}
const o7 = { c: new C(7) };
st(o7, raw);
console.log(rd(o7));

class K2 {
  c = new C(8);
  get() {
    return this.c.x;
  }
}
const k2 = new K2();
st(k2, raw);
console.log(k2.get());
