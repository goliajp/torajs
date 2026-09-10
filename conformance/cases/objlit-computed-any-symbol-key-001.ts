// `{ [k]: v }` where `k` is typed `any` and holds a Symbol at
// runtime. The dynobj lane coerced every non-Symbol-TYPED key with
// ToString, which throws for a Symbol; §7.1.19 ToPropertyKey lets a
// Symbol through as itself and stringifies the rest.
const sym: any = Symbol("s");
const o: any = { [sym]: 1, plain: 2 };
console.log(o[sym], o.plain, Object.getOwnPropertySymbols(o).length, Object.keys(o));
const f: any = Object.freeze({ [sym]: 3 });
console.log(f[sym], Object.isFrozen(f));
const n: any = 7;
const p: any = { [n]: "seven", [true as any]: "t", [null as any]: "nul", [undefined as any]: "und" };
console.log(p[7], p["7"], p.true, p.null, p.undefined, Object.keys(p));
const keyed: any = { [Symbol.iterator]: 0 };
console.log(typeof keyed[Symbol.iterator], Object.keys(keyed));
function make(k: any, v: any) { return { [k]: v }; }
const m1 = make(sym, "a"), m2 = make("b", 2), m3 = make(1.5, 3);
console.log((m1 as any)[sym], Object.keys(m1).length, (m2 as any).b, (m3 as any)["1.5"]);
const objKey: any = { toString() { return "custom"; } };
const q: any = { [objKey]: 9 };
console.log(q.custom, Object.keys(q));
