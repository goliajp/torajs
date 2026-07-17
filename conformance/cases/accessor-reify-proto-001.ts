// RFC 20260718-accessor-reify 刀 1 — Object.prototype.__proto__ is a real
// accessor own entry (Annex B §B.2.2.1): gOPD answers { get, set, E0, C1 },
// the faces are callable builtin cells, set runs the silent-invalid /
// refusal-throws semantics, and %Object.prototype% is immutable-prototype.
const d: any = Object.getOwnPropertyDescriptor(Object.prototype, "__proto__");
console.log("has-desc", d !== undefined);
console.log("get-type", typeof d.get, "set-type", typeof d.set);
console.log("enum", d.enumerable, "conf", d.configurable);
console.log("has-value", "value" in d, "has-writable", "writable" in d);
console.log("get-name", d.get.name);
console.log("set-name", d.set.name);
console.log("get-len", d.get.length, "set-len", d.set.length);

// get.call — ordinary / custom / null-proto / primitive receivers
const o = { a: 1 };
console.log("get-ordinary", d.get.call(o) === Object.prototype);
const parent = { z: 42 };
const child: any = Object.create(parent);
console.log("get-custom", d.get.call(child) === parent);
console.log("get-nullproto", d.get.call(Object.create(null)) === null);
console.log("get-num", d.get.call(1) === Number.prototype);
console.log("get-root", d.get.call(Object.prototype) === null);

// get abrupt — undefined / null receivers throw TypeError
let g1 = "no-throw";
try { d.get.call(undefined); } catch (e: any) { g1 = (e instanceof TypeError) ? "TypeError" : "other"; }
console.log("get-undef-throws", g1);
let g2 = "no-throw";
try { d.get.call(null); } catch (e: any) { g2 = (e instanceof TypeError) ? "TypeError" : "other"; }
console.log("get-null-throws", g2);

// set.call — real link write
const c2: any = {};
console.log("set-ret", d.set.call(c2, parent));
console.log("set-linked", c2.z);
console.log("set-gpo", Object.getPrototypeOf(c2) === parent);

// set silent — invalid value / primitive receiver
const c3: any = {};
console.log("set-bool-val", d.set.call(c3, true));
console.log("set-bool-val-unchanged", Object.getPrototypeOf(c3) === Object.prototype);
console.log("set-num-recv", d.set.call(1, parent));

// set abrupt — nullish receiver throws before everything
let s1 = "no-throw";
try { d.set.call(undefined, parent); } catch (e: any) { s1 = (e instanceof TypeError) ? "TypeError" : "other"; }
console.log("set-undef-throws", s1);
let s2 = "no-throw";
try { d.set.call(null, parent); } catch (e: any) { s2 = (e instanceof TypeError) ? "TypeError" : "other"; }
console.log("set-null-throws", s2);

// %Object.prototype% is an immutable-prototype exotic object
let im = "no-throw";
try { d.set.call(Object.prototype, {}); } catch (e: any) { im = (e instanceof TypeError) ? "TypeError" : "other"; }
console.log("set-immutable-throws", im);
console.log("set-immutable-same-null", d.set.call(Object.prototype, null));
console.log("root-still-null", Object.getPrototypeOf(Object.prototype) === null);

// reflection neighbors — gOPN lists it, member read still answers the link
console.log("gopn-has", Object.getOwnPropertyNames(Object.prototype).indexOf("__proto__") >= 0);
console.log("keys-skips", Object.keys(Object.prototype).indexOf("__proto__") < 0);
console.log("member-read-root", Object.prototype.__proto__ === null);
console.log("member-read-child", ({} as any).__proto__ === Object.prototype);
console.log("lookup-getter", ({} as any).__lookupGetter__("__proto__") === d.get);
