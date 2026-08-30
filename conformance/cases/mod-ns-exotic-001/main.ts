// §10.4.6 — a module namespace is an EXOTIC object, and the ordinary
// object literal the resolver minted answered every question about it
// wrong: extensible, entries configurable, no `@@toStringTag`, own
// keys in walk order.
//
// Five of the internal methods are not new behavior once the object
// carries the right attributes — §10.4.6.3 [[IsExtensible]] and
// §10.4.6.4 [[PreventExtensions]] off the non-extensible bit (which
// is also what makes §10.4.6.6 refuse a fresh key), §10.4.6.5
// [[GetOwnProperty]]'s `configurable: false` off the per-entry seal,
// §10.4.6.11 [[OwnPropertyKeys]]'s trailing symbol key and the
// `[object Module]` badge off the `@@toStringTag` own entry
// (§10.4.6.12 step 8), and §10.4.6.2 [[SetPrototypeOf]] refusing a
// non-null V.
//
// The sorted key order is §10.4.6.12 step 7: the resolver discovers
// exports in walk order (zeta, alpha, mid, default) and the namespace
// answers them by code unit.
import * as ns from "./lib";

console.log(Object.getOwnPropertyNames(ns).join(","));
console.log(Object.keys(ns).join(","));
console.log(Reflect.ownKeys(ns).map(String).join(","));
console.log(Object.isExtensible(ns), Reflect.isExtensible(ns));
console.log(Object.prototype.toString.call(ns));
console.log(String(ns[Symbol.toStringTag]));
console.log(JSON.stringify(Object.getOwnPropertyDescriptor(ns, "alpha")));
console.log(Reflect.defineProperty(ns, "fresh", { value: 1 }));
console.log(Reflect.setPrototypeOf(ns, {}));
console.log(ns.alpha, ns.mid(), ns.default, ns.zeta);
