// §10.5.7-§10.5.10 — a trap's answer must not contradict a
// non-configurable property or a non-extensible target.
const run = (label: string, f: () => any) => {
  try { console.log(label, "->", f()); } catch (e: any) { console.log(label, "throws", e.constructor.name); }
};
const t: any = {};
Object.defineProperty(t, "fixed", { value: 1, writable: false, configurable: false });
Object.defineProperty(t, "noget", { set(_v: any) {}, configurable: false });
Object.defineProperty(t, "noset", { get() { return 2; }, configurable: false });
Object.defineProperty(t, "loose", { value: 3, writable: true, configurable: true });
const liar: any = new Proxy(t, {
  get() { return 99; },
  set() { return true; },
  has() { return false; },
  deleteProperty() { return true; },
});
run("get fixed", () => liar.fixed);
run("get noget", () => liar.noget);
run("get loose", () => liar.loose);
run("set fixed same", () => Reflect.set(liar, "fixed", 1));
run("set fixed diff", () => Reflect.set(liar, "fixed", 5));
run("set noset", () => Reflect.set(liar, "noset", 5));
run("set loose", () => Reflect.set(liar, "loose", 5));
run("has fixed", () => "fixed" in liar);
run("has loose", () => "loose" in liar);
run("has absent", () => "absent" in liar);
run("delete fixed", () => Reflect.deleteProperty(liar, "fixed"));
run("delete loose", () => Reflect.deleteProperty(liar, "loose"));
const sealedTarget: any = { a: 1 };
Object.preventExtensions(sealedTarget);
const p2: any = new Proxy(sealedTarget, { has() { return false; }, deleteProperty() { return true; } });
run("has non-extensible", () => "a" in p2);
run("delete non-extensible", () => Reflect.deleteProperty(p2, "a"));
run("has non-extensible absent", () => "b" in p2);
const honest: any = new Proxy(t, { get(tt: any, k: any) { return tt[k]; } });
run("honest get", () => honest.fixed + honest.noset);
