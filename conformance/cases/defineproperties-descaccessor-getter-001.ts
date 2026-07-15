// RFC 20260716 刀 23 — Object.defineProperties(obj, props) Phase 1
// accessor spec-Get. Test262 cluster: 15.2.3.7-3-{4,8} /
// 15.2.3.7-5-a-5 / 15.2.3.5-4-19. `props` carries an accessor entry
// via `Object.defineProperty(props, "prop", {get, enumerable: true})`;
// spec §20.1.2.3.1 step 3.b [[Get]](props, key) invokes the getter
// and passes the OWNED result through ToPropertyDescriptor. Before
// this blade the raw `AccessorPair` fell out of the accept gate as
// a `Tag::AccessorPair` cell (18), producing spurious §6.2.6.5 step-1
// TypeErrors instead of applying the getter's returned descriptor.

// Case A — accessor returns `{}` (empty dynobj) — test262 15.2.3.7-3-4
// / 15.2.3.5-4-19. Dispatcher then finds no fields → default empty
// descriptor. `hasOwn("prop") === true`.
const objA: any = {};
const propsA: any = {};
Object.defineProperty(propsA, "prop", { get: () => ({}), enumerable: true });
Object.defineProperties(objA, propsA);
console.log("A hasOwn:", objA.hasOwnProperty("prop"));

// Case B — accessor returns `{set: function() {}}` — test262 15.2.3.7-5-a-5.
// Getter body has no `get` field → the produced accessor pair binds a
// set-only accessor; `hasOwn` is true and `typeof obj.prop === "undefined"`.
const objB: any = {};
const propsB: any = {};
Object.defineProperty(propsB, "prop", { get: () => ({ set: function () {} }), enumerable: true });
Object.defineProperties(objB, propsB);
console.log("B hasOwn:", objB.hasOwnProperty("prop"), "typeof:", typeof objB.prop);

// Case C — accessor without a getter (undefined [[Get]] → spec
// §10.1.8.1 step 3 returns undefined) is rejected by
// ToPropertyDescriptor step 1 (not an object) → TypeError.
const objC: any = {};
const propsC: any = {};
Object.defineProperty(propsC, "prop", { set: function () {}, enumerable: true });
try {
  Object.defineProperties(objC, propsC);
  console.log("C did not throw");
} catch (e) {
  console.log("C caught:", (e as Error).message);
}

// Case D — accessor returns a primitive (`"abc"`) → not an object →
// TypeError (same gate as case C).
const objD: any = {};
const propsD: any = {};
Object.defineProperty(propsD, "prop", { get: () => "abc", enumerable: true });
try {
  Object.defineProperties(objD, propsD);
  console.log("D did not throw");
} catch (e) {
  console.log("D caught:", (e as Error).message);
}
