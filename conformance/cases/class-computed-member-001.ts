// RFC 20260802-class-computed-member 刀 2 — runtime computed class
// member names: §15.4 ComputedPropertyName evaluates at class-
// definition time (ToPropertyKey), and the member installs on
// C.prototype / C under the runtime key.
//
// p1: instance computed method — any-lane call + prototype reflection.
const k = "m";
class C1 { [k]() { return 1 + 2; } }
const c1 = new C1();
console.log((c1 as any).m());
console.log(typeof (C1.prototype as any).m);

// p2: static computed method.
const sk = "sm";
class C2 { static [sk]() { return 42; } }
console.log((C2 as any).sm());

// p3: computed accessor pair — same runtime key merges get + set
// (test262 accessor-names/computed.case shape).
var _;
var stringSet;
class C3 {
  get [_ = "str" + "ing"]() { return "get string"; }
  set [_ = "str" + "ing"](param) { stringSet = param; }
}
console.log((C3.prototype as any)["string"]);
(C3.prototype as any)["string"] = "set string";
console.log(stringSet);

// p4: key expressions evaluate once each, in declaration order,
// across the instance / static split.
const log: string[] = [];
function key(n: string) { log.push(n); return n; }
class C4 { [key("a")]() { return 1; } static [key("b")]() { return 2; } get [key("c")]() { return 3; } }
console.log(log.join(","));
console.log((new C4() as any).a(), (C4 as any).b(), (C4.prototype as any)["c"]);

// p5: a Symbol key installs a symbol-keyed prototype entry
// (§7.1.19 step 2 pass-through).
const s = Symbol("x");
class C5 { [s]() { return 7; } }
console.log(Object.getOwnPropertySymbols(C5.prototype).length);
console.log((C5.prototype as any)[s]());
