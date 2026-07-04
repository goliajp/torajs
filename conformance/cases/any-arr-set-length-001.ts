const a: any = [1, 2, 3];
a[0] = 9;
console.log(a);
console.log(a.length);
a[1] = "s";
console.log(a);
const t: number[] = [7, 8, 9];
const b: any = t;
b[2] = 5;
console.log(b);
console.log(t[2]);
console.log(b.length);
const f: number[] = [1.5, 2.5];
const c: any = f;
c[0] = 4;
console.log(c);
console.log(c.length);
const s: any = "hello";
console.log(s.length);
const s2: any = "hi";
console.log(s2.length);
const o: any = { length: 42 };
console.log(o.length);
const o2: any = { k: 1 };
console.log(o2.length);
try {
  const z: any = null;
  console.log(z.length);
} catch (err) {
  console.log("null len caught");
}
