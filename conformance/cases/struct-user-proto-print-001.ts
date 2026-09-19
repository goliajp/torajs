// A re-parented struct: iteration, inherited name/length, coercion and the
// console.log prefix all follow its user [[Prototype]].
const it = { *[Symbol.iterator]() { yield 1; yield 2; } };
const o = { own: 0 };
Object.setPrototypeOf(o, it);
console.log([...(o as any)].join(","));
for (const v of o as any) console.log("v", v);
const nm = { name: "inherited", length: 7 };
const s = { k: 1 };
Object.setPrototypeOf(s, nm);
console.log((s as any).name, (s as any).length);
class C { m() { return 1; } }
class D { toString() { return "D!"; } }
const c: any = new C();
Object.setPrototypeOf(c, D.prototype);
console.log(String(c), `${c}`, c.constructor === D);
console.log(c);
const n = { a: 1 };
Object.setPrototypeOf(n, null);
console.log(n);
console.log(Object.prototype.toString.call(c));
const c2: any = new C();
Object.setPrototypeOf(c2, null);
console.log(c2);
const c3: any = new C();
Object.setPrototypeOf(c3, { tag: 1 });
console.log(c3);
