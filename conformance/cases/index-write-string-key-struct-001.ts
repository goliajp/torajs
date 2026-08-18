// cluster #3 (rotation 442): dynamic String / Symbol / Any keys on a
// class-instance (struct) receiver's keyed WRITE ride the keyed set
// kernel — the read side has admitted this domain since chunk 753.
class K {
  get [1 + 1]() { return 2; }
  set [1 + 1](v: any) { console.log("iset", v); }
  static get [1 + 1]() { return 3; }
  static set [1 + 1](v: any) { console.log("sset", v); }
}
const c = new K();
console.log(c[String(1 + 1)]);
c[String(1 + 1)] = 5;
console.log(c[String(1 + 1)] = 6);
console.log(K[String(1 + 1)]);
K[String(1 + 1)] = 7;

// a plain instance takes a dynamic string key as an expando store
class P { x: number = 1; }
const p = new P();
const k: string = "dyn" + "k";
p[k] = 41;
console.log(p[k]);

// an Any-typed key with an element spelling dispatches through the
// same kernel (ToPropertyKey at runtime)
const anyKey: any = "2";
const c2 = new K();
c2[anyKey] = 8;

// a symbol key stores its own cell, uncoerced (§7.1.19 step 2)
const sym: any = Symbol("s");
p[sym] = 42;
console.log(p[sym]);
