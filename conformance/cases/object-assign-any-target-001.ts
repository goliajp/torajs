const o: any = {};
Object.assign(o, { y: 2 });
console.log(o.y);
const o2: any = { a: 1 };
const r: any = Object.assign(o2, { b: "two" }, { c: 3, a: 9 });
console.log(r.a, r.b, r.c);
console.log(r === o2);
// nullish source skips
Object.assign(o2, null, undefined);
console.log(Object.keys(o2).length);
// nullish target throws
let caught = "";
try { Object.assign(null as any, { x: 1 }); } catch (e: any) { caught = e.name; }
console.log(caught);
// getter source contributes its answer
const src: any = {};
Object.defineProperty(src, "g", { get: function () { return 42; }, enumerable: true });
const t2: any = {};
Object.assign(t2, src);
console.log(t2.g);
// non-enumerable source props skip
const src2: any = { vis: 1 };
Object.defineProperty(src2, "hid", { value: 9, enumerable: false });
const t3: any = {};
Object.assign(t3, src2);
console.log(t3.vis, t3.hid);
