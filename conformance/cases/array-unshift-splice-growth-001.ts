// unshift and splice past capacity, on typed and any arrays, from empty
// and from small lists: every step past a growth boundary keeps order

function sum(xs: number[]): number {
  let s = 0;
  for (let i = 0; i < xs.length; i++) s += xs[i] * (i + 1);
  return s;
}

const a: number[] = [];
for (let i = 0; i < 40; i++) a.unshift(i);
console.log(a.length, a[0], a[39], sum(a));

const b: number[] = [1, 2, 3];
for (let i = 0; i < 20; i++) b.splice(1, 0, i, i + 100);
console.log(b.length, b[0], b[1], b[2], b[b.length - 1], sum(b));

const c: any[] = [];
for (let i = 0; i < 37; i++) c.unshift(i % 3 === 0 ? "s" + i : i);
console.log(c.length, c[0], c[36], c.join(",").length);

const d: any[] = ["x"];
for (let i = 0; i < 25; i++) d.splice(d.length, 0, i, { k: i });
console.log(d.length, d[0], d[1], d[50].k, d[49]);

const e: any = [];
for (let i = 0; i < 33; i++) e.unshift(i);
console.log(e.length, e[0], e[32]);

const f: string[] = [];
for (let i = 0; i < 18; i++) f.unshift("w" + i);
f.splice(9, 0, "m1", "m2", "m3");
console.log(f.length, f[0], f[9], f[11], f[12], f[20]);
