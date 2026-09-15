// §10.5.1 / §10.5.2 — a prototype trap may say anything about an
// extensible target, but a non-extensible target's [[Prototype]] is
// fixed and the trap must agree with it.
const run = (label: string, f: () => any) => {
  try { console.log(label, "->", f()); } catch (e: any) { console.log(label, "throws", e.constructor.name); }
};
const other = { other: true };
const open: any = {};
const closed: any = {};
Object.preventExtensions(closed);
const lie = { getPrototypeOf() { return other; }, setPrototypeOf() { return true; } };
const pOpen: any = new Proxy(open, lie);
const pClosed: any = new Proxy(closed, lie);
run("get open", () => Object.getPrototypeOf(pOpen) === other);
run("get closed", () => Object.getPrototypeOf(pClosed) === other);
run("instanceof closed", () => pClosed instanceof Array);
run("set open", () => Reflect.setPrototypeOf(pOpen, other));
run("set closed other", () => Reflect.setPrototypeOf(pClosed, other));
run("set closed same", () => Reflect.setPrototypeOf(pClosed, Object.prototype));
const truthful: any = new Proxy(closed, { getPrototypeOf(t: any) { return Object.getPrototypeOf(t); } });
run("get closed truthful", () => Object.getPrototypeOf(truthful) === Object.prototype);
