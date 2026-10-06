// struct array literals whose elements are all typed keep the typed
// layout even when the element shapes differ: a missing optional
// field, another field order, a nullable field set one way and the
// other, a subclass instance ahead of its base

interface A { x: number; y?: string }
interface Q { x: number; y: number }
interface N { v: number; next: N | null }
class C { constructor(public a: number) {} }
class D extends C { constructor(a: number, public b: number) { super(a); } }
const xs: A[] = [{ x: 1, y: "a" }, { x: 2 }];
console.log(xs[0].x, xs[0].y, xs[1].x, xs.length);
const qs: Q[] = [{ x: 1, y: 2 }, { y: 3, x: 4 }];
console.log(qs[1].x, qs[1].y);
const n1: N = { v: 1, next: null };
const ns: N[] = [{ v: 0, next: null }, { v: 2, next: n1 }];
console.log(ns[1].next === null ? "-" : ns[1].next.v, ns[0].next);
const cs: C[] = [new D(1, 2), new C(3)];
console.log(cs[0].a, cs[1].a, cs[0] instanceof D);
let sum = 0;
for (const q of qs) sum += q.x * q.y;
console.log(sum);
function inFn(): number {
  const local: Q[] = [{ x: 5, y: 6 }, { y: 7, x: 8 }];
  return local[0].x * local[1].y;
}
console.log(inFn());
