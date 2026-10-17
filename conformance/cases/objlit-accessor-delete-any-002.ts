// The data property a store creates over a deleted accessor member is
// an ordinary own property on every face (descriptor, for-in,
// Object.values / entries, inspect), a later store updates it, and a
// second delete removes it again — the accessor does not come back.
// `undefined` is a value it can hold like any other.
const o = { get n() { return 7 }, set n(v: number) { console.log("setter", v) }, k: 1 };
const a: any = o;
delete a.n;
console.log(a.hasOwnProperty("n"), Object.getOwnPropertyDescriptor(a, "n"));
a.n = 5;
console.log(a.hasOwnProperty("n"), JSON.stringify(a));
console.log(Object.getOwnPropertyDescriptor(a, "n"));
const ks: string[] = [];
for (const key in a) ks.push(key);
console.log(ks.join(","), Object.values(a).join(","), JSON.stringify(Object.entries(a)));
console.log(a);
a.n = 6;
console.log(a.n, o.n);
console.log(delete a.n, a.n, o.n, "n" in a, Object.keys(a).join(","));
console.log(delete a.n, a.n);
a.n = undefined;
console.log(a.n, "n" in a, Object.keys(a).join(","), JSON.stringify(a));
delete a.n;
a.k = 2;
a.n = "s";
console.log(a.n, o.n, Object.keys(a).join(","));
