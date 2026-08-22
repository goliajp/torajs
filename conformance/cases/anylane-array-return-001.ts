// A typed array crossing into the any world keeps its element kind.
function named() { return ["x", "yy"]; }
const g: any = named;
const a: any = g();
console.log(a.length, a[0], a[1], JSON.stringify(a));

const lit: any = { m() { return ["p", "q"]; } };
const b: any = lit.m();
console.log(b.length, b[0], JSON.stringify(b));

const arrow: any = () => ["u", "v"];
const d: any = arrow();
console.log(d.length, d[0], JSON.stringify(d));

// Numeric and boolean element kinds too.
const nums: any = (() => [1, 2, 3]) as any;
const n: any = nums();
console.log(n[0], n[2], JSON.stringify(n));

const flags: any = (() => [true, false]) as any;
const fl: any = flags();
console.log(fl[0], fl[1], JSON.stringify(fl));

const dbls: any = (() => [1.5, 2.5]) as any;
const dd: any = dbls();
console.log(dd[0], dd[1], JSON.stringify(dd));

// Nested arrays keep the whole chain.
const nested: any = (() => [["a"], ["b"]]) as any;
const nn: any = nested();
console.log(JSON.stringify(nn), nn[0][0]);

// And the typed reader still sees the same array.
console.log(named()[1]);

// for-of over the boxed result.
const seen: string[] = [];
for (const s of a) seen.push(String(s));
console.log(seen.join("|"));
