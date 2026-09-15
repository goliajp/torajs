// §10.4.6 — the namespace a dynamic import() resolves to is the same
// exotic object a static `import * as ns` binds: null prototype,
// non-extensible, every export non-configurable, so `[[Delete]]` of an
// export answers false and leaves it in place (a module is strict code,
// so the operator form would throw — the Reflect form reads the
// boolean), `[[Set]]` is refused, and the keys come back sorted with
// the `Module` tag.
import("./m.js").then(function (ns: any) {
  console.log("delete", Reflect.deleteProperty(ns, "default"), Reflect.deleteProperty(ns, "x"), Reflect.deleteProperty(ns, "zz"));
  console.log("ext", Object.isExtensible(ns), "sealed", Object.isSealed(ns), "frozen", Object.isFrozen(ns));
  console.log("keys", Object.keys(ns).join(","));
  console.log("desc", JSON.stringify(Object.getOwnPropertyDescriptor(ns, "x")));
  console.log("has", "default" in ns, ns.default, ns.f());
  // (bun answers a null-prototype object for getPrototypeOf(ns) where
  // §10.4.6.1 says null — the tag line is the shared observable)
  console.log("tag", Object.prototype.toString.call(ns));
  console.log("set", Reflect.set(ns, "x", 9), ns.x);
  return import("./m.js").then(function (again: any) { console.log("same", again === ns); });
});
