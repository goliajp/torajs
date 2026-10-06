// a cell created in another order than its declared layout, handed
// through `any` to a typed parameter: it fits the slot as it is, so
// the body sees the same object (writes land, identity holds, keys
// keep the creation order) whether the call reaches an all-any twin
// or the boxed adapter alone
type A = { b: string; x: number };
function setX(p: A): A {
  p.x = 9;
  return p;
}
const f: any = setX;
const a: A = { x: 1, b: "q" };
const r = f(a);
console.log(r === a, a.x, Object.keys(r));

// receiver channel: a method reached through an `any` receiver
class K {
  put(p: A): A {
    p.b = "w";
    return p;
  }
}
const k: any = new K();
const a2: A = { x: 2, b: "r" };
const r2 = k.put(a2);
console.log(r2 === a2, a2.b, Object.keys(r2), JSON.stringify(r2));

// a nested closure escaping through `any`
function mk(): any {
  const bump = (p: A): A => {
    p.x = p.x + 1;
    return p;
  };
  return bump;
}
const g = mk();
const a3: A = { x: 3, b: "s" };
const r3 = g(a3);
console.log(r3 === a3, a3.x, Object.keys(r3));

// an in-order cell of the same type through the same channels
const a4: A = { b: "t", x: 4 };
const r4 = f(a4);
console.log(r4 === a4, a4.x, Object.keys(r4));
