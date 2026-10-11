// An `any[]` parameter that only reads its array leaves a typed array
// handed to it typed; one that writes through it in any way, or hands
// it on, sees values of any kind stored into the caller's array.
function total(xs: any[]) {
  let s = 0;
  for (const v of xs) s += v;
  for (let i = 0; i < xs.length; i++) s += xs[i];
  return s + xs.indexOf(2) + xs.map((v) => v * 2).join("").length;
}
const a: number[] = [1, 2, 3];
console.log(total(a), a.join(","), a[0] + 0.5);

function cut(xs: any[]) {
  xs.length = 1;
}
const b: number[] = [4, 5];
cut(b);
console.log(b.length, b.join(","));

function splice(xs: any[]) {
  xs.splice(1, 0, "mid");
}
const c: number[] = [1, 2];
splice(c);
console.log(c.join(","), typeof c[1]);

function fill(xs: any[]) {
  xs.fill(null);
}
const d: number[] = [1, 2];
fill(d);
console.log(d.join(","), d[0]);

function relay(xs: any[]) {
  splice(xs);
}
const e: number[] = [9, 8];
relay(e);
console.log(e.join(","));

function viaCallback(xs: any[]) {
  xs.forEach((v, i, arr) => {
    if (i === 0) arr.push("cb");
  });
}
const f: number[] = [1];
viaCallback(f);
console.log(f.join(","), f.length);

function viaClosure(xs: any[]) {
  const add = () => xs.unshift("front");
  add();
}
const g: number[] = [2];
viaClosure(g);
console.log(g.join(","), g.length);

function swap(xs: any[]) {
  const t = xs[0];
  xs[0] = xs[1];
  xs[1] = t;
}
const h: number[] = [1.5, 2];
swap(h);
console.log(h.join(","), h[0] + h[1]);
