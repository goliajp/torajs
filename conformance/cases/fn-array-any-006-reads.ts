// More reads of an element of a typed function array holding an `any`
// function: element-returning methods behind `!`, destructuring, an
// optional call, a field or a return slot handed the element, a nested
// declaration as the callback, and a `reduce` seeded with an element.
type F = (x: number) => number;
const tag: any = (x: any) => "t" + x;
const mk = (): F[] => {
  const hs: F[] = [(x: number) => x + 1, (x: number) => x * 2, (x: number) => x - 3];
  hs[0] = tag;
  hs[2] = tag;
  return hs;
};

const hs = mk();
const f1 = hs.at(0)!;
const f1b: F = hs.at(-1)!;
console.log(f1(1), f1b(2));
const f2 = hs.find((f) => f(1) === 2)!;
const f3 = hs.findLast((f) => true)!;
console.log(f2(4), f3(5));
const f4 = mk().pop()!;
const f5 = mk().shift()!;
console.log(f4(6), f5(7));

const [a, b] = hs;
console.log(a(1), b(1));
console.log(hs[0]?.(2), hs[1]?.(2));

const o: { f: F } = { f: (x: number) => x };
o.f = hs[0];
console.log(o.f(1));
class C {
  g: F = (x: number) => x;
}
const c = new C();
c.g = hs[2];
console.log(c.g(3));

function first(): F {
  return hs[0];
}
console.log(first()(2));
const r = first();
console.log(r(3));

function outer(k: number) {
  function each(f: F) {
    console.log(f(k));
  }
  hs.forEach(each);
  function pick(acc: F, f: F): F {
    return k > 0 ? f : acc;
  }
  console.log(hs.reduce(pick)(k + 1));
}
outer(3);

console.log(hs.reduce((f, g) => f, hs[0])(4));
const z = hs.reduce((f, g) => f, hs[2]);
console.log(z(5));
