// Same-named bindings in sibling or nested blocks are separate
// bindings: an escaped element stored in one keeps its kind there,
// while the other keeps doing number arithmetic. An `any[]` element
// stored in a `number` binding keeps its kind too.
const d: number[] = [1, 2, 3];
const u: any = d;
u[1] = "s";

function sums(): void {
  {
    let sum = 0;
    for (const v of d) sum = sum + v;
    console.log(sum, typeof sum);
  }
  {
    let sum = 0.5;
    for (let i = 0; i < 3; i++) sum = sum + i;
    console.log(sum, typeof sum);
  }
}
sums();

let x = 10;
{
  let x = 0;
  x = x + d[1];
  console.log(x);
}
x = x * 2;
console.log(x, typeof x);

function callbacks(): void {
  {
    let tot = 0;
    d.forEach((v: number) => {
      tot = tot + v;
    });
    console.log(tot);
  }
  {
    let tot = 0;
    [4, 5].forEach((v: number) => {
      tot = tot + v;
    });
    console.log(tot, typeof tot);
  }
}
callbacks();

const xs: any[] = [1, "s"];
let n: number = 0;
for (let i = 0; i < xs.length; i++) n = n + xs[i];
console.log(n);
let e: number = 0;
for (const v of xs) e = e + v;
console.log(e);
let k: number = 0;
k = k * xs[0];
console.log(k, typeof k);

// a retyped binding still reaches every typed use the program makes
function f(x: number): number;
function f(x: string): string;
function f(x: any): any {
  return typeof x === "number" ? x * 2 : x + "!";
}
let t = 0;
t = t + d[1];
const r = f(t);
console.log(r, typeof r);
console.log(Math.max(t, 3), `${t}`, String(t).length, t === "0s");
const parts = [t, t];
console.log(parts.length, parts[0]);
