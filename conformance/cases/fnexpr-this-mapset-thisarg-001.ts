// fn-expr `this` as a Map / Set forEach callback with a thisArg
// (§24.1.3.5 / §24.2.3.7 step 5 — T is the callback's this)
const m: any = new Map();
m.set("a", 1);
m.set("b", 2);
const out: any = [];
m.forEach(function (v: any, k: any) { out.push(k + ":" + (v * this.mul)); }, { mul: 5 });
console.log(out.join(","));

const s: any = new Set([10, 20]);
const out2: any = [];
s.forEach(function (v: any) { out2.push(v + this.off); }, { off: 1 });
console.log(out2.join(","));

// this-free callback keeps the plain ABI; no-thisArg binds undefined
const out3: any = [];
m.forEach(function (v: any, k: any) { out3.push(k + String(v) + String(this === undefined)); });
console.log(out3.join(","));
