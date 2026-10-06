// an `as` cast from `any` to a struct type keeps the value as it is,
// including when the cast binding lands in a struct array

interface A { x: number; y: string }
const j: any = JSON.parse('{"x": 2, "y": "b"}');
const xs: A[] = [{ x: 1, y: "a" }, j];
console.log(xs[0].x, xs[0].y);
console.log(xs[1].x, xs[1].y);
const k: any = { y: "q", x: 4 };
const bs: A[] = [{ x: 1, y: "a" }, k as any as A];
console.log(bs[1].x, bs[1].y);
const one = j as A;
const cs: A[] = [one];
console.log(cs[0].x, cs[0].y, cs[0] === j);
const v1: any = { y: "a", x: 1 };
const a1 = v1 as any as A;
console.log(a1.x, a1.y);
const v3: any = { x: 5 };
const a3 = v3 as A;
console.log(a3.x, a3.y);
const v4: any = 42;
const a4 = v4 as A;
console.log(a4.x, a4.y);
const ds: A[] = [a3, a4];
console.log(ds.map((d) => d.x).join(","));
