// Double casts and casts inside function bodies reach the any world
// the same way a single `as any` does.
const c: number[] = [3, 1, 2];
(c as unknown as any).push("u");
for (const v of c) console.log(v);

const d: number[] = [9];
(d as object as any)[1] = "o";
console.log(d.length, d[1]);

function g(): number {
  const a: number[] = [5, 6];
  (a as any).push("t");
  (a as any)[3] = 1.5;
  console.log(a);
  return a.length;
}
console.log(g());

function h(a: number[]): void {
  (a as any).push("p");
}
const x: number[] = [1, 2];
h(x);
console.log(x.length, x[0], x[2]);

// arrays that never reach the any world keep their typed behavior
const plain: number[] = [3, 1, 2];
const other: number[] = [4, 5];
(other as any).push("w");
plain.push(9);
console.log(plain, other);
let total = 0;
for (let i = 0; i < plain.length; i++) total += plain[i];
console.log(total);
