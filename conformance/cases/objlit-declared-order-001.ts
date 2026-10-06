// a literal spelled in another order than its declared struct type:
// reads use the declared layout, enumeration follows creation order
type A = { b: string; x: number };
type In = { y: string; z: number };
type H = { b: In; x: number };

function show(tag: string, a: A): void {
  console.log(tag, a.b, a.x);
  console.log(JSON.stringify(a));
  console.log(Object.keys(a).join(","));
  console.log(JSON.stringify(Object.values(a)), JSON.stringify(Object.entries(a)));
  const ks: string[] = [];
  for (const k in a) ks.push(k);
  console.log(ks.join(","));
  console.log({ ...a });
  console.log(JSON.stringify({ ...a }));
  console.log(a);
}

const a1: A = { x: 1, b: "q" };
show("const", a1);
let a2: A = { x: 2, b: "r" };
show("let", a2);
a2 = { x: 3, b: "s" };
show("assign", a2);
show("arg", { x: 4, b: "t" });
function mk(n: number): A {
  return { x: n, b: "u" };
}
show("return", mk(5));
const h: H = { x: 6, b: { z: 7, y: "v" } };
console.log(h.b.y, h.b.z, h.x);
console.log(JSON.stringify(h));
console.log(Object.keys(h.b).join(","));
console.log(h);
class Holder {
  a: A = { b: "init", x: 0 };
}
const hd = new Holder();
hd.a = { x: 8, b: "w" };
show("field", hd.a);
const inOrder: A = { b: "k", x: 9 };
show("inorder", inOrder);
const sp = { ...a1, y: 1 };
console.log(JSON.stringify(sp), Object.keys(sp).join(","));
const sp2 = { y: 1, ...a1 };
console.log(JSON.stringify(sp2), Object.keys(sp2).join(","));
const anyA: any = a1;
console.log(anyA.b, anyA.x, JSON.stringify(anyA), Object.keys(anyA).join(","));
const arr: A[] = [a1, inOrder];
console.log(JSON.stringify(arr));
// class prefix subtyping still compares by position
class Base {
  b = "";
}
class Sub extends Base {
  x = 0;
}
const s = new Sub();
s.b = "sb";
s.x = 3;
const base: Base = s;
console.log(base.b, JSON.stringify(s), Object.keys(s).join(","));
function takeBase(p: Base): string {
  return p.b;
}
console.log(takeBase(s));
