// RFC 20260802-class-computed-member 刀 1 — §12.7.6 PropertyName
// literal forms as class member names, and whole-literal computed
// keys folding to the same static names (§7.1.19 ToPropertyKey at
// compile time).
//
// p1: string-literal method / accessor pair / field, numeric-literal
//     method (hex → canonical "16"), static string method.
class C {
  "m"() { return 1; }
  get "g"() { return 3; }
  set "g"(v: number) { console.log("set", v); }
  0x10() { return 16; }
  static "sm"() { return 2; }
  "f" = 5;
}
const c = new C();
console.log(c.m());
console.log(c.g);
c.g = 7;
console.log((C as any).sm());
console.log((c as any).f);
console.log((C.prototype as any)["16"]());

// p2: whole-literal computed keys — `["w"]` / `[42]` / accessor
// `get ["cg"]` fold exactly like their direct-literal spellings.
class D { ["w"]() { return 9; } [42]() { return 42; } get ["cg"]() { return 8; } }
const d = new D();
console.log(d.w());
console.log(d.cg);
console.log((D.prototype as any)["42"]());

// p3: the string key "constructor" IS the constructor (§15.4.3
// PropName equivalence — same key, same slot).
class E { x = 0; "constructor"() { this.x = 99; } }
console.log(new E().x);

// p4: reserved-word spellings via literals ('default' accessor,
// 'if' method) and a non-canonical numeric key (1.0 → "1").
class F { get "default"() { return 4; } "if"() { return 5; } [1.0]() { return 6; } }
const f = new F();
console.log(f.default);
console.log(f.if());
console.log((F.prototype as any)["1"]());
