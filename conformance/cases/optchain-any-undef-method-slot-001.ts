// An `any` holding undefined / null written into a slot whose object
// type has a method stays nullish: `?.` short-circuits, `===` sees it.
type T = { m: () => number };
type U = { m(): number };

const p: T | undefined = undefined as any;
console.log(p?.m(), p?.m?.(), p?.["m"](), p === undefined, typeof p);
const q: T | null = null as any;
console.log(q?.m(), q === null);
const u: U | undefined = undefined as any;
console.log(u?.m());

function local() {
  const p: T | undefined = undefined as any;
  return String(p?.m()) + ":" + (p === undefined);
}
console.log(local());

let l: T | undefined = { m: () => 3 };
console.log(l?.m());
l = undefined;
console.log(l?.m());

function ret(): T | undefined {
  return undefined as any;
}
console.log(ret()?.m());

class C {
  p: T | undefined = undefined as any;
}
console.log(new C().p?.m());

function param(p: T | undefined) {
  return p?.m();
}
console.log(param({ m: () => 4 }), param(undefined as any));

let side = 0;
const r: T | undefined = undefined as any;
console.log(r?.m(side++), side);
const live: T | undefined = { m: () => 9 } as T;
console.log(live?.m());
