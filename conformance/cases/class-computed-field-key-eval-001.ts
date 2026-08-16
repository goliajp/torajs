// 419-01 — a class's computed member name is ToPropertyKey'd at the
// class-definition point (ES §15.7.14 / §7.1.19), not at whatever
// later site happens to consume the key. The FIELD lane used to park
// the raw value in the key global and let the ctor-prefix keyed write
// convert it, so `toString` ran once per construction and a throwing
// one escaped a class that was never constructed.
let log: string[] = [];
let k1 = { toString: function() { log.push("k1"); return "a" } };
let k2 = { toString: function() { log.push("k2"); return "b" } };

log.push("before");
class D {
  [k1] = 1;
  [k2] = 2;
}
log.push("after");
console.log(log.join(","));

let d1: any = new D();
let d2: any = new D();
console.log(log.join(","));
console.log(d1["a"], d1["b"], d2["a"], d2["b"]);

// A throwing key aborts the class definition itself.
let bad = { toString: function() { throw new Error("boom") } };
let seen = "none";
try {
  class E { [bad]; }
  seen = "no-throw";
} catch (e: any) {
  seen = e.message;
}
console.log(seen);

// Symbol keys take §7.1.19 step 2 — passed through, never stringified.
let s = Symbol("tag");
class F { [s] = 7; }
let f: any = new F();
console.log(f[s], Object.keys(f).length);

// Static field + method faces keep answering (they already converted
// at the class-decl position; this guards the shared key global).
let k3 = { toString: function() { return "c" } };
class G {
  static [k3] = 3;
  [k1]() { return "m" }
}
console.log((G as any)["c"], (new G() as any)["a"]());
