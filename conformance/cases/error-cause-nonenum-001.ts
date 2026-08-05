// ES §20.5.8.1 InstallErrorCause installs `cause` with
// CreateNonEnumerableDataPropertyOrThrow — `{W:1, E:0, C:1}`. An
// assignment cannot express that, so the ctor-installed entry and a
// user's own post-construction assignment differ, and both spellings
// have to keep their own answer.
const a = new Error("m", { cause: 42 });
console.log("ctor keys:", JSON.stringify(Object.keys(a)));
console.log("ctor json:", JSON.stringify(a));
const aa: any = a;
console.log("ctor read:", aa.cause);

// §20.5.8.1 tests HasProperty, not a defined value: an explicit
// undefined cause is installed, and stays non-enumerable.
const u = new Error("m", { cause: undefined });
console.log("undef keys:", JSON.stringify(Object.keys(u)));
console.log("undef json:", JSON.stringify(u));

// No options at all — nothing installed.
const n = new Error("m");
console.log("none keys:", JSON.stringify(Object.keys(n)));
console.log("none json:", JSON.stringify(n));

// A user's own assignment is an ordinary enumerable data property.
const w = new Error("m");
const wa: any = w;
wa.cause = 7;
console.log("user keys:", JSON.stringify(Object.keys(w)));
console.log("user json:", JSON.stringify(w));
console.log("user read:", wa.cause);

// Subclasses forward `options` to Error's ctor, so they inherit the
// attributes rather than repeating the install.
const t = new TypeError("t", { cause: "x" });
console.log("sub keys:", JSON.stringify(Object.keys(t)));
console.log("sub json:", JSON.stringify(t));
const ta: any = t;
console.log("sub read:", ta.cause);
class W extends Error {
  constructor(m: string, o?: any) {
    super(m, o);
  }
}
const uw = new W("w", { cause: 1 });
console.log("user-sub keys:", JSON.stringify(Object.keys(uw)));
const uwa: any = uw;
console.log("user-sub read:", uwa.cause);

// A cause carrying a heap value survives the transfer into the entry.
const h = new Error("m", { cause: { k: [1, 2] } });
const ha: any = h;
console.log("heap json:", JSON.stringify(ha.cause));
console.log("heap keys:", JSON.stringify(Object.keys(h)));

// for-in agrees with Object.keys on both shapes.
const s1: string[] = [];
for (const k in a) { s1.push(k); }
console.log("ctor forin:", JSON.stringify(s1));
const s2: string[] = [];
for (const k in w) { s2.push(k); }
console.log("user forin:", JSON.stringify(s2));
