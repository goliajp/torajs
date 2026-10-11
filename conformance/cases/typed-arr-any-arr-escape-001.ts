// A typed array handed to a slot annotated `any[]` is written values of
// any kind through it; every holder sees the same array afterwards.
const f = (acc: any[], x: number) => {
  acc.push("s");
  return [x];
};
const k: number[] = [3];
f(k, 1);
console.log(k.join(","), k.length, typeof k[1]);

function setFirst(xs: any[]): void {
  xs[0] = "t";
}
const m: number[] = [5, 6];
setFirst(m);
console.log(m.join(","), m[0] + 1, m[1] + 1);

const n: number[] = [1, 2];
const view: any[] = n;
view.push(true);
console.log(n.length, n[2], n.join("|"));

const seed: number[] = [0];
const r = [1, 2].reduce((acc: any[], x: number) => {
  acc.push(x % 2 === 0 ? "even" : x);
  return acc;
}, seed);
console.log(r.join(","), seed.length);

const box: { items: any[] } = { items: [] };
const typed: number[] = [7];
box.items = typed;
box.items.push("y");
console.log(typed.join(","));
