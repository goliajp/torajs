// an `any` value placed in a struct array literal keeps its own
// representation: reads and writes reach the object itself, whatever
// its shape

interface A { x: number; y: string }
function show(p: A) { console.log("show", p.x, p.y); }
function sum(xs: A[]): number { let s = 0; for (const e of xs) s += e.x; return s; }
function run(j: any, k: any, n: any) {
  const xs: A[] = [{ x: 1, y: "a" }, j];
  console.log(xs[0].x, xs[0].y, xs[1].x, xs[1].y, xs.length);
  console.log(xs[1].x * 10 + 1, `${xs[1].y}!`);
  xs[1].x = 99;
  console.log(j.x, xs[1].x === j.x, xs[1] === j);
  show(xs[1]);
  console.log(sum(xs));
  console.log(xs.map((e) => e.y).join(","));
  xs.forEach((e) => console.log("each", e.x));
  console.log(xs.filter((e) => e.x > 50).length);
  const bs: A[] = [{ x: 1, y: "a" }, k as any as A];
  console.log(bs[1].x, bs[1].y);
  const odd: A[] = [{ x: 5, y: "f" }, n];
  console.log(odd[1].x, odd[1].y, typeof odd[1]);
}
run(JSON.parse('{"x": 2, "y": "b"}'), { y: "q", x: 4 }, 42);
const tj: any = { x: 3, y: "t" };
const top: A[] = [{ x: 0, y: "z" }, tj];
console.log(top[1].x, top[1].y);
function readTop(): string { return top[1].y + top[0].y; }
console.log(readTop());
const missing: any = { x: 8 };
const ms: A[] = [missing, { x: 9, y: "nine" }];
console.log(ms[0].x, ms[0].y, ms[1].y);
