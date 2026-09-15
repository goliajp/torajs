// §10.5.8 / §10.5.9 — a trap-less [[Get]] / [[Set]] forwards to the
// target WITH the receiver, and a Proxy met on an ordinary object's
// prototype chain is handed the original receiver.
const target: any = {
  get self() { return this; },
  set store(v: any) { this.stored = v; },
};
const p: any = new Proxy(target, { get: null, set: undefined } as any);
console.log("getter this is proxy", p.self === p);
const child: any = Object.create(new Proxy(target, {}));
console.log("inherited getter this is child", child.self === child);
const pp: any = new Proxy(new Proxy(target, {}), {});
console.log("double proxy getter this", pp.self === pp);

const log: string[] = [];
const traced: any = new Proxy({ foo: 1 }, {
  getOwnPropertyDescriptor(t: any, k: any) { log.push("gopd:" + k); return Reflect.getOwnPropertyDescriptor(t, k); },
  defineProperty(t: any, k: any, d: any) { log.push("define:" + k + ":" + Object.keys(d).join("/")); return Reflect.defineProperty(t, k, d); },
});
traced.foo = 2;
traced.bar = 3;
console.log(log.join(" "), traced.foo, traced.bar);

let seenReceiver: any;
const trapped: any = new Proxy({}, { set(_t: any, _k: any, _v: any, r: any) { seenReceiver = r; return true; } });
const heir: any = Object.create(trapped);
heir.prop = "value";
console.log("set trap sees heir", seenReceiver === heir, Object.hasOwn(heir, "prop"));

const refusing: any = new Proxy({}, { set() { return false; } });
const heir2: any = Object.create(refusing);
try {
  heir2.x = 1;
  console.log("no throw");
} catch (e: any) {
  console.log(e instanceof TypeError ? "TypeError" : "other");
}

const ro: any = {};
Object.defineProperty(ro, "k", { value: 1, writable: false, configurable: true });
console.log("non-writable on receiver", Reflect.set({}, "k", 5, new Proxy(ro, {})), ro.k);
const inner: any = {};
const innerP: any = new Proxy(inner, { set(t: any, k: any, v: any, r: any) { t[k] = v * 10; return r === outerP; } });
const outerP: any = new Proxy(innerP, {});
outerP.n = 4;
console.log("proxy target set trap with receiver", inner.n);
