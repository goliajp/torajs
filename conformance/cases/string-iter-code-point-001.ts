// §22.1.5.1 — every string iteration steps per CODE POINT: a surrogate
// pair is one element. The typed for-of already did; the any lane's
// for-of / spread / Array.from, the reified String.prototype[@@iterator],
// and array destructuring of a string (which now takes the iterator
// lane — a rest binds an Array, not `.slice`'s string) read one code
// unit each before RFC 20260915.
const s = "a😀b";
const sa: any = s;
const out: any[] = [];
for (const c of sa) out.push(c, c.length);
console.log("f", JSON.stringify(out));
console.log("sp", [...sa].length, Array.from(sa).length, Array.from(sa)[1].length);
const it = s[Symbol.iterator]();
console.log("it", it.next().value, it.next().value.length, it.next().value, it.next().done);
const [a, ...r] = "xyz";
console.log("d1", a, JSON.stringify(r));
const [e1, e2] = "😀b";
console.log("d2", e1, e2, e1.length);
for (const [h, ...t] of ["ab", "😀c"]) console.log("d3", h, JSON.stringify(t));
let [q1, , q3 = "d"] = "xy";
console.log("d4", q1, q3);
const [w1, w2] = new String("😀!") as any;
console.log("d5", w1.length, w2);
const sub = "xx😀yy".slice(2);
for (const c of sub as any) out.push(c.length);
console.log("sub", JSON.stringify(out.slice(6)));
async function fa() { const acc: any[] = []; for await (const c of sa) acc.push(c); console.log("fa", JSON.stringify(acc)); }
fa();
function* g() { for (const [x, ...y] of ["😀z"]) { yield x.length; yield y.length; } }
console.log("g", [...g()]);
