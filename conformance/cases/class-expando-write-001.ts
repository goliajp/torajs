// Rotation 373 — class-instance dynamic member writes: the checker's
// write-side admission of names the layout never carries (mirror of
// the read side's RFC 20260804 blade-4 miss arm), riding the RFC
// 20260714-struct-dynamic-props runtime expando dict. Private names
// keep §13.15.2 PutValue → PrivateSet semantics: a METHOD or a
// getter-only accessor write throws a catchable TypeError. Also
// covers the throw-info fix: a fn whose only throw source is a
// member store (frozen gate / dynamic lane) propagates to the caller.

// 1. method-body expando write + read
class D {
  method() {
    this._v = 42;
    return this._v;
  }
}
console.log("m-expando", new D().method());

// 2. constructor expando + later read through another method
class E {
  constructor() {
    this._w = 7;
  }
  get() {
    return this._w;
  }
}
console.log("ctor-expando", new E().get());

// 3. class WITH declared fields grows an expando next to them
class F {
  x = 1;
  method() {
    this._v = 5;
    return this.x + this._v;
  }
}
console.log("declared+expando", new F().method());

// 4. the t262 private-setter shape: accessor body writes `this._v`,
//    reached through an arrow fn capturing lexical this
var C = class {
  set #m(v: any) {
    this._v = v;
  }
  method() {
    let arrowFunction = () => {
      this.#m = "Test262";
    };
    arrowFunction();
  }
};
let c = new (C as any)();
c.method();
console.log("priv-setter-body", c._v);

// 5. compound accumulate on an expando slot (undefined || 0 seed)
class G {
  bump() {
    this._c = (this._c || 0) + 1;
    return this._c;
  }
}
const g = new G();
g.bump();
console.log("expando-accum", g.bump());

// 6. private METHOD plain write — PrivateSet kind=method TypeError
class M1 {
  #m() {
    return 7;
  }
  trip() {
    this.__x = 0; // unrelated expando first, still throws below
    this.#m = 9;
  }
}
try {
  new M1().trip();
  console.log("m-write no throw");
} catch (e) {
  console.log("m-write", (e as Error).constructor.name);
}

// 7. private getter-only accessor: plain write and compound both throw
class A1 {
  get #g() {
    return 5;
  }
  plain() {
    this.#g = 9;
  }
  comp() {
    return this.#g + 1;
  }
  compWrite() {
    this.#g += 1;
  }
}
try {
  new A1().plain();
  console.log("g-plain no throw");
} catch (e) {
  console.log("g-plain", (e as Error).constructor.name);
}
try {
  new A1().compWrite();
  console.log("g-comp no throw");
} catch (e) {
  console.log("g-comp", (e as Error).constructor.name);
}

// 8. control: private FIELD compound assignment stays a normal store
class P1 {
  #f = 1;
  inc() {
    return (this.#f += 1);
  }
}
console.log("field-comp", new P1().inc());

// 9. control: private getter+setter PAIR write goes through the setter
class P2 {
  _backing = 0;
  get #v() {
    return this._backing;
  }
  set #v(n: number) {
    this._backing = n;
  }
  set(n: number) {
    this.#v = n;
    return this.#v;
  }
}
console.log("pair-write", new P2().set(11));

// 10. frozen instance: method-body store throws AND propagates to the
//     caller (the throw-info fix — this swallowed before)
class FR {
  x = 0;
  m() {
    this.x = 5;
  }
}
const fr = new FR();
Object.freeze(fr);
try {
  fr.m();
  console.log("frozen no throw");
} catch (e) {
  console.log("frozen", (e as Error).constructor.name, fr.x);
}

// 11. non-extensible instance refuses NEW expando keys, keeps updates
class NE {
  m(v: number) {
    this._k = v;
  }
}
const ne = new NE();
ne.m(1);
Object.preventExtensions(ne);
ne.m(2); // update of a live expando key stays allowed
console.log("ne-update", (ne as any)._k);
try {
  (ne as any)._fresh = 1;
  console.log("ne-fresh no throw");
} catch (e) {
  console.log("ne-fresh", (e as Error).constructor.name);
}

// 12. enumeration sees the expando key next to declared fields
class K {
  a = 1;
  m() {
    this.b = 2;
  }
}
const k = new K();
k.m();
console.log("keys", JSON.stringify(Object.keys(k)));
