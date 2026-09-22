// §7.1.19 ToPropertyKey: step 1 is ToPrimitive(key, string), and a Symbol
// answer IS the key (step 2). A Symbol wrapper always answers its Symbol
// (§20.4.3.5), and so does a toString returning one — every keyed read,
// write, `in`, hasOwnProperty and descriptor lookup must key by it.
const w: any = Object(Symbol("w"));
const obj: any = {};
function t(l: string, f: () => unknown) { try { console.log(l, String(f())); } catch (e) { console.log(l, "THROW", (e as any).message); } }
t("in", () => w in obj);
t("hasOwn", () => obj.hasOwnProperty(w));
t("get", () => obj[w]);
t("gopd", () => Object.getOwnPropertyDescriptor(obj, w));
t("set", () => (obj[w] = "ok"));
t("get2", () => obj[w]);
t("in2", () => w in obj);
t("hasOwn2", () => obj.hasOwnProperty(w));
t("inc", () => obj[w]++);
const k2: any = { toString() { return Symbol.for("tk"); } };
const o2: any = {};
o2[k2] = 5;
console.log(o2[Symbol.for("tk")], Object.keys(o2).length, Object.getOwnPropertySymbols(o2).length);
const p: any = new Proxy({}, {});
p[w] = 1;
console.log(p[w], Object.getOwnPropertySymbols(p).length);
