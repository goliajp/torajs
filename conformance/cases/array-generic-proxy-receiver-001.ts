// §23.1.3 "intentionally generic" over a Proxy receiver — `length`, the
// per-index Get / HasProperty / Set / DeleteProperty all go through the
// handler's traps, each once per spec step.
const log: string[] = [];
const target: any = { 0: "a", 1: "b", 2: "c", length: 3 };
const handler: any = {
  get(t: any, k: any, r: any) { log.push("get " + String(k)); return Reflect.get(t, k, r); },
  has(t: any, k: any) { log.push("has " + String(k)); return Reflect.has(t, k); },
  set(t: any, k: any, v: any, r: any) { log.push("set " + String(k)); return Reflect.set(t, k, v, r); },
  deleteProperty(t: any, k: any) { log.push("delete " + String(k)); return Reflect.deleteProperty(t, k); },
};
const p: any = new Proxy(target, handler);
const flush = (label: string, v: any) => { console.log(label, JSON.stringify(v), log.join(",")); log.length = 0; };
flush("includes", Array.prototype.includes.call(p, "b"));
flush("indexOf", Array.prototype.indexOf.call(p, "c"));
flush("join", Array.prototype.join.call(p, "-"));
flush("forEach", (Array.prototype.forEach.call(p, () => {}), 0));
flush("push", Array.prototype.push.call(p, "d"));
flush("pop", Array.prototype.pop.call(p));
flush("reverse", Array.prototype.reverse.call(p) === p);
console.log(JSON.stringify(target));
const thrower: any = new Proxy({ length: 2, 0: 1, 1: 2 }, { has() { throw new RangeError("has"); } });
try {
  Array.prototype.copyWithin.call(thrower, 0, 1);
} catch (e: any) {
  console.log(e instanceof RangeError, e.message);
}
