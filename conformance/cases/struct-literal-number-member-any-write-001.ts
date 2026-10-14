// A member typed by a number literal lives in a number slot like a
// `number` member: a number written through the any lane fits it, and
// its reads see that number; a value of another kind makes its reads
// answer that value.

type T = { k: 1 };
const t: T = { k: 1 };
(t as any).k = 2.5;
const x = t.k;
console.log(typeof x, x);
console.log(t.k + 1, t.k === 1, `${t.k}`);

type U = { k: 1; n: number; s: "a"; b: true };
const u: U = { k: 1, n: 0, s: "a", b: true };
(u as any).n = 7.25;
(u as any).s = 3;
(u as any).b = "yes";
const n = u.n;
const s = u.s;
const b = u.b;
console.log(typeof n, n, typeof s, s, typeof b, b);
console.log(JSON.stringify(u), Object.values(u).join(","));
const spread = { ...u };
console.log(spread.k, spread.n, spread.s, spread.b);

class C {
  k: 2 = 2;
  m: number = 1;
}
const c = new C();
(c as any).k = 9.5;
(c as any).m = "m";
const ck = c.k;
console.log(typeof ck, ck, c.k * 2, typeof c.m, c.m);
let total = 0;
for (let i = 0; i < 3; i++) total += c.k;
console.log(total);

type V = { k: 3 };
const v: V = { k: 3 };
const w: any = v;
w.k = "three";
const vk = v.k;
console.log(typeof vk, vk, v.k === 3);
