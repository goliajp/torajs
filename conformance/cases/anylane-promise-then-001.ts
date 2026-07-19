// any-lane Promise.prototype.then / .catch (RFC
// 20260720-anylane-promise-methods knife 2): the boxed-adapter
// callback bridge over the typed promise kernel, boxing the settled
// value per the cell's value_repr stamp (knife 1). Legs: chain /
// cb-throw-to-rejection / non-callable pass-through / 2-arg then /
// every repr (str f64 bool null void heap-arr) / user thenable
// dynobj keeps its own then / microtask ordering.
// chain
const q1: any = Promise.resolve(1);
q1.then((v: any) => v * 10).then((v: any) => { console.log("chain", v); });
// cb throw -> rejection
const q2: any = Promise.resolve(2);
q2.then((v: any) => { throw "boom" + v; }).catch((e: any) => { console.log("caught", e); });
// non-callable handler pass-through
const q3: any = Promise.resolve(3);
q3.then("nope").then((v: any) => { console.log("passthru", v); });
// 2-arg then on rejected
const q4: any = Promise.reject("bad4");
q4.then((v: any) => { console.log("never", v); }, (e: any) => { console.log("onerr", e); });
// value reprs
const s: any = Promise.resolve("hi");
s.then((v: any) => { console.log("str", v); });
const f: any = Promise.resolve(1.5);
f.then((v: any) => { console.log("f64", v); });
const b: any = Promise.resolve(true);
b.then((v: any) => { console.log("bool", v); });
const n: any = Promise.resolve(null);
n.then((v: any) => { console.log("null", v); });
const u: any = Promise.resolve();
u.then((v: any) => { console.log("void", v); });
const arr: any = Promise.resolve([1, 2]);
arr.then((v: any) => { console.log("heap", v[0] + v[1]); });
// user thenable dynobj keeps its own then
const o: any = { then: (cb: any) => cb(99) };
o.then((v: any) => { console.log("user-then", v); });
console.log("sync");
