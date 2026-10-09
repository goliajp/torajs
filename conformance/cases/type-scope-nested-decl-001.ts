// A type declared in a function or a block is scoped there and shadows
// an outer declaration of the same name.
type K = string;
function f(): number {
  type K = number;
  const x: K = 41;
  return x + 1;
}
const s: K = "a";
console.log(f(), s + "b");
{
  type K = boolean;
  const b: K = true;
  console.log(b === true);
}

function area(w: number, h: number): number {
  interface Rect {
    w: number;
    h: number;
  }
  type Area = (r: Rect) => number;
  const calc: Area = (r) => r.w * r.h;
  const r: Rect = { w, h };
  return calc(r);
}
console.log(area(3, 4));

function sum(xs: number[]): number {
  type Acc = { total: number; count: number };
  let acc: Acc = { total: 0, count: 0 };
  for (const x of xs) {
    type Step = number;
    const d: Step = x;
    acc = { total: acc.total + d, count: acc.count + 1 };
  }
  return acc.total / acc.count;
}
console.log(sum([1, 2, 3, 6]));

const label = (n: number): string => {
  type K = number;
  const k: K = n * 10;
  return "k" + k;
};
console.log(label(4));

function pick(tag: number): string {
  switch (tag) {
    case 1:
      type L = string;
      return "one";
    default:
      const l: L = "other";
      return l;
  }
}
console.log(pick(1), pick(2));
