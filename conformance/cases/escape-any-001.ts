// L3b #2 primary shape — typed alias escapes into any, kind transition visible
const t: number[] = [10, 20];
const u: any = t;
u[0] = "s";
console.log(t[0]);
console.log(u[0]);
console.log(t);
console.log(t[0] + 1);
console.log(t.length);
for (const x of t) console.log(x);
console.log(t.indexOf(20));
console.log(t.join(","));
// edge 5: return typed arr into any binding
function mk(): number[] { return [9, 10]; }
const u5: any = mk();
u5[0] = "q";
console.log(u5[0]);
// non-escaping arrays keep raw behavior
const w: number[] = [7, 8];
w[0] = 70;
console.log(w[0] + w[1], w);
// edge 2: escaped binding passed to typed param (alias class demotes callee too)
function sum(a: number[]): number {
  let s = 0;
  for (const x of a) s = s + x;
  return s;
}
const t2: number[] = [3, 4];
const u2: any = t2;
console.log(sum(t2));
u2[0] = "y";
console.log(t2[0]);
// edge 3: escape via any-container store
const t3: number[] = [5, 6];
const box: any[] = [];
box.push(t3);
(box[0] as any)[0] = "z";
console.log(t3[0]);
