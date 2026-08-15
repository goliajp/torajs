// Rotation 410 — `class K extends null` (§15.7): legal to define;
// K.prototype's [[Prototype]] is null, the class object keeps
// %Function.prototype%, and `new` throws (the implicit/explicit
// super call reaches a non-constructor).
class K extends null {}
console.log(Object.getPrototypeOf(K.prototype));
console.log(Object.getPrototypeOf(K) === Function.prototype);
try {
  new (K as any)();
  console.log("constructed");
} catch (e: any) {
  console.log("implicit-super", e.constructor.name);
}

class A extends null {
  constructor() {
    super();
  }
}
try {
  new (A as any)();
} catch (e: any) {
  console.log("explicit-super", e.constructor.name);
}

// §9.2.2 step 13 return-override: a derived ctor returning an object
// never needs its this binding, so the class is constructible
class R extends null {
  constructor() {
    super();
    return Object.assign({}, { tag: "override" });
  }
}
try {
  console.log("return-override", (new (R as any)() as any).tag);
} catch (e: any) {
  console.log("return-override-threw", e.constructor.name);
}
