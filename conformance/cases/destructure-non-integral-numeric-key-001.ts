const xs: number[] = [1, 2, 3];
const { 1.5: v, 1: w } = xs as any;
console.log(v, w);
const o = { 1.5: "a", 2: "b", 1e21: "big" };
const { 1.5: p, 2: q, 1e21: r } = o;
console.log(p, q, r);
function f({ 2.5: a, 0: b }: any): string {
  return String(a) + "/" + String(b);
}
console.log(f({ 2.5: "x", 0: "y" }));
console.log(f(["z"]));
