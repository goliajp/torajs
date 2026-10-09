// a function stored in a fn-typed slot whose return or parameter is an
// object type hands its fractional number fields through the slot as
// numbers: the slot's signature widens the object face the way it
// widens a number or array face
type A = { right: number; left: number };
const mk = (): A => ({ right: 0.5, left: 2 });
const f: () => A = mk;
console.log(f().right, f().left);

function make(): A {
  return { right: 1.25, left: 3 };
}
const g: () => A = make;
console.log(g().right + g().left);

function via(h: () => A): number {
  return h().right * 2;
}
console.log(via(mk), via(make));

const sum: (a: A) => number = (a: A) => a.right + a.left;
console.log(sum({ right: 0.75, left: 1 }));

type Outer = { inner: { w: number }; n: number };
const mo: () => Outer = () => ({ inner: { w: 4.5 }, n: 1 });
console.log(mo().inner.w, mo().n);

type Tool = { scale: number; apply: (x: number) => number };
const mt: () => Tool = () => ({ scale: 1.5, apply: (x: number) => x * 1.5 });
const tool = mt();
console.log(tool.apply(2), tool.scale);

let acc = 0;
for (let i = 0; i < 1000; i++) acc += f().right;
console.log(acc);
