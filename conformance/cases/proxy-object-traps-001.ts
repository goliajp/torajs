// §10.5.1-4 — the four object-level internal methods.
const base: any = { kind: "base" };
const t: any = Object.create(base);

const log: string[] = [];
const p: any = new Proxy(t, {
  getPrototypeOf(target: any) { log.push("gpo"); return base; },
  setPrototypeOf(target: any, proto: any) { log.push("spo"); return true; },
  isExtensible(target: any) { log.push("ext"); return Object.isExtensible(target); },
  preventExtensions(target: any) { log.push("pe"); Object.preventExtensions(target); return true; },
});

console.log(Object.getPrototypeOf(p) === base);
console.log(Reflect.getPrototypeOf(p) === base);
console.log(Object.isExtensible(p));
console.log(Reflect.setPrototypeOf(p, null));
Object.preventExtensions(p);
console.log(Object.isExtensible(p), Object.isExtensible(t));
console.log(log.join(","));

// Trap-less proxies forward all four.
const q: any = new Proxy(Object.create(base), {});
console.log(Object.getPrototypeOf(q) === base, Object.isExtensible(q));
Object.preventExtensions(q);
console.log(Object.isExtensible(q));

// A getPrototypeOf trap must answer an object or null.
const bad: any = new Proxy({}, { getPrototypeOf() { return 1 as any; } });
try { Object.getPrototypeOf(bad); } catch (e: any) { console.log("gpo bad:", e instanceof TypeError); }

// §10.5.3 step 8 — the isExtensible trap must agree with the target.
const lying: any = new Proxy({}, { isExtensible() { return false; } });
try { Object.isExtensible(lying); } catch (e: any) { console.log("ext lie:", e instanceof TypeError); }

// §10.5.4 step 8 — reporting success while the target is extensible.
const lying2: any = new Proxy({}, { preventExtensions() { return true; } });
try { Object.preventExtensions(lying2); } catch (e: any) { console.log("pe lie:", e instanceof TypeError); }

// A setPrototypeOf trap that refuses.
const stubborn: any = new Proxy({}, { setPrototypeOf() { return false; } });
console.log(Reflect.setPrototypeOf(stubborn, null));
try { Object.setPrototypeOf(stubborn, null); } catch (e: any) { console.log("spo refuse:", e instanceof TypeError); }
