// A handler that is itself a proxy can revoke the proxy from inside
// the trap LOOKUP — the spec captured target/handler before that, so
// the internal method must survive it.
function createProxy(proxyTarget: any): any {
  const r: any = Proxy.revocable(proxyTarget, new Proxy({}, {
    get() { r.revoke(); return undefined; },
  }));
  return r.proxy;
}

console.log(Object.getPrototypeOf(createProxy({})) === Object.prototype);
console.log(Object.getPrototypeOf(createProxy([])) === Array.prototype);
console.log(Object.isExtensible(createProxy({})));
console.log(Object.isExtensible(createProxy(Object.preventExtensions({}))));
console.log(Object.getOwnPropertyDescriptor(createProxy({}), "a"));
console.log(Object.getOwnPropertyDescriptor(createProxy({ a: 5 }), "a").value);
console.log("a" in createProxy({}), "a" in createProxy({ a: 5 }));
console.log(createProxy({}).a, createProxy({ a: 5 }).a);

const o1: any = {};
Object.setPrototypeOf(createProxy(o1), Array.prototype);
console.log(Object.getPrototypeOf(o1) === Array.prototype);

const o2: any = {};
Object.preventExtensions(createProxy(o2));
console.log(Object.isExtensible(o2));

const o3: any = {};
Object.defineProperty(createProxy(o3), "a", { value: 5 });
console.log(o3.a);

// The proxy is revoked by the time [[Set]] would land, so a strict
// assignment raises the revoked TypeError.
try { createProxy({}).a = 0; } catch (e: any) { console.log("set:", e instanceof TypeError); }
