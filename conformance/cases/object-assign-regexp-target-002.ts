// Object.assign onto a RegExp copies symbol keys after string keys
// (§20.1.2.1 step 4.c walks OwnPropertyKeys: integer keys, strings in
// insertion order, then symbols), and a copied @@replace / @@split is
// what the String methods then call. A key that RegExp.prototype owns
// only as a getter refuses the [[Set]], and Object.assign throws: the
// sources before it have landed, the ones after it have not.
const sm = Symbol("m");
const src: any = { 2: "two", b: "bee", 1: "one", a: "ay" };
src[sm] = "sym";
src[Symbol.replace] = (s: string, rep: string) => "custom:" + s + ":" + rep;
const r = /a/;
const out = Object.assign(r, src);
console.log(out === r, "xay".replace(r, "Q"), (r as any)[sm], Object.keys(r).join(","));
const order: string[] = [];
const seen: any = new Proxy(
  {},
  {
    set(t: any, k: any, v: any) {
      order.push(String(k));
      t[k] = v;
      return true;
    },
  },
);
Object.assign(seen, src);
console.log(order.join(","));
const r2 = /b/g;
Object.assign(r2, { [Symbol.split]: () => ["S"] });
console.log("abc".split(r2).join("|"));

const cases: [string, any][] = [
  ["global", { global: true }],
  ["flags", { flags: "g" }],
  ["source", { source: "x" }],
  ["sticky", { sticky: true }],
  ["lastIndex", { lastIndex: 3 }],
];
for (const [name, s] of cases) {
  const re = /c/;
  try {
    Object.assign(re, s);
    console.log(name, "ok", re.lastIndex, re.global, re.flags, re.source);
  } catch (e) {
    console.log(name, (e as Error).constructor.name, re.global, re.source);
  }
}
const t = /d/;
try {
  Object.assign(t, { tag: 1 }, { flags: "g" }, { late: 2 });
} catch (e) {
  console.log("multi", e instanceof TypeError, (t as any).tag, (t as any).late, t.flags);
}
try {
  (t as any).global = true;
} catch (e) {
  console.log("assign", e instanceof TypeError, t.global);
}
console.log(Reflect.set(t, "flags", "g"), JSON.stringify(t.flags));
const m = new Map<string, number>();
try {
  (m as any).size = 3;
} catch (e) {
  console.log("size", e instanceof TypeError, m.size);
}
console.log(Reflect.set(m, "size", 3), m.size);
