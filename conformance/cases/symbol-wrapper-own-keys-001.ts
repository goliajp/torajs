// A Symbol wrapper (`Object(sym)`) keeps its expando properties in the
// same +16 bag the Number / String / Boolean wrappers use, and every
// reader of that bag listed only those three — so `Object.keys` on a
// symbol wrapper answered [] while `w.x` read back the value.
const w: any = Object(Symbol("q"));
w.x = 2; w.y = "s"; Object.defineProperty(w, "hidden", { value: 1, enumerable: false });
console.log(Object.keys(w), Object.getOwnPropertyNames(w), Reflect.ownKeys(w).length, JSON.stringify(Object.entries(w)), JSON.stringify(Object.values(w)));
console.log(typeof w, w.x, w.y, "x" in w, w.hasOwnProperty("y"), JSON.stringify(w));
const n: any = Object(1); n.a = 1; const b: any = Object(true); b.a = 2; const s: any = Object("ab"); s.a = 3;
console.log(Object.keys(n), Object.keys(b), Object.keys(s));
for (const k in w) console.log("forin", k);
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(w, "hidden")), Object.isFrozen(w), Object.isExtensible(w));
Object.freeze(w); try { w.z = 1; } catch (e) { console.log("frozen", (e as Error).message); } console.log(Object.keys(w), Object.isFrozen(w));
const m: any = new Map(); m.a = 1; const r: any = /x/g; r.b = "s"; const d: any = new Date(0); d.c = 3;
console.log(JSON.stringify(m), JSON.stringify(r), JSON.stringify(new Map()), JSON.stringify({ m, r }), JSON.stringify(d));
