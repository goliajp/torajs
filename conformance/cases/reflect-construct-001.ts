// §28.1.2 Reflect.construct (r293) — IsConstructor gates on target
// and newTarget, unconditional CreateListFromArrayLike, the
// factory-adapter construct. (The differing-newTarget [[Prototype]]
// re-wire is a recorded boundary: fixed-layout instances refuse
// SetPrototypeOf loudly — kernel module doc.)
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

const p: any = Reflect.construct(Point, [3, 4]);
console.log(p.x, p.y, p instanceof Point);

// explicit newTarget === target
const q: any = Reflect.construct(Point, [5, 6], Point);
console.log(q.x, q.y);

// argumentsList reads array-like (§7.3.18)
const arrLike: any = { length: 2, 0: 7, 1: 8 };
const r: any = Reflect.construct(Point, arrLike);
console.log(r.x, r.y);

// non-constructor target — TypeError
try {
  Reflect.construct(((n: number) => n) as any, []);
  console.log("no-throw");
} catch (e: any) {
  console.log("caught", e instanceof TypeError);
}

// nullish argumentsList — TypeError (no empty-list amnesty)
try {
  Reflect.construct(Point, undefined as any);
  console.log("no-throw2");
} catch (e: any) {
  console.log("caught2", e instanceof TypeError);
}

// primitive argumentsList — TypeError
try {
  Reflect.construct(Point, 5 as any);
  console.log("no-throw3");
} catch (e: any) {
  console.log("caught3", e instanceof TypeError);
}
