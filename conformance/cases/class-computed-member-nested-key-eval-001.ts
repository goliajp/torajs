// 420-01 — a class written inside a function evaluates its computed
// member names where it is written, once per call. The hoist used to
// lift such a class to the top level, and the class-decl-position key
// evaluation went with it — landing after the LAST top-level statement,
// since hoisted classes append there. Routing a class with a computed
// member name through the in-place lane keeps the evaluation put.
let log: string[] = [];
let k = { toString: function() { log.push("key"); return "m" } };

function make(): string {
  log.push("enter");
  class C {
    [k]() { return "called" }
  }
  let c: any = new C();
  log.push("leave");
  return c["m"]();
}

log.push("top");
console.log(make(), make());
console.log(log.join(","));

// The evaluation is inside whatever guards the class, so a throwing
// key is catchable at the class's own position.
let bad = { toString: function() { throw new Error("boom") } };
let seen = "none";
try {
  class D { [bad]() {} }
  seen = "no-throw";
} catch (e: any) {
  seen = e.message;
}
console.log(seen);

// Accessors and static fields take the same route.
let log2: string[] = [];
let gk = { toString: function() { log2.push("gk"); return "g" } };
let sk = { toString: function() { log2.push("sk"); return "s" } };
function make2(): string {
  class E {
    static [sk] = "S";
    get [gk]() { return "G" }
  }
  return (E as any)["s"] + (new E() as any)["g"];
}
console.log(make2(), make2());
console.log(log2.join(","));
