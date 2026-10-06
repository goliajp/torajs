// a struct array literal with an `any` element passed to a struct
// array parameter, assigned to a struct array binding, or spreading an
// `any[]`; and an `as` cast from `any` bound under its annotation

interface A { x: number; y: string }
function take(xs: A[]) {
  console.log(xs[0].x, xs[1].x, xs[1].y, xs.length);
}
const j: any = JSON.parse('{"x": 6, "y": "six"}');
take([{ x: 1, y: "a" }, j]);
console.log(j.x);
let ys: A[] = [{ x: 0, y: "o" }];
console.log(ys[0].y);
ys = [{ x: 0, y: "o" }, j];
console.log(ys[1].x, ys[1].y, ys[1] === j);
ys[1].y = "changed";
console.log(j.y);
function local(v: any) {
  let zs: A[] = [];
  zs = [v, { x: 9, y: "nine" }];
  console.log(zs[0].x, zs[1].y, zs.map((z) => z.x).join(","));
}
local({ x: 5, y: "five" });
local(3);
const one: A = j as A;
console.log(one.x, one.y, one === j);
const spread: A[] = [{ x: 1, y: "s" }, ...([j] as any[])];
console.log(spread[1].x, spread.length);
