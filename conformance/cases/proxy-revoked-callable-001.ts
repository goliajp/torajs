// §10.5.14 ProxyCreate step 7 — a proxy's [[Call]] / [[Construct]]
// come from its target at creation, so revoking it later does not turn
// a function proxy into an object.
function kind(f: () => unknown): string {
  try {
    return "ok " + String(f());
  } catch (e: any) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}
const fnRev = Proxy.revocable(function () { return 1; }, {});
const objRev = Proxy.revocable({}, {});
console.log("before", typeof fnRev.proxy, typeof objRev.proxy);
fnRev.revoke();
objRev.revoke();
console.log("after", typeof fnRev.proxy, typeof objRev.proxy);
console.log("wrapping", typeof new Proxy(fnRev.proxy, {}), typeof new Proxy(objRev.proxy, {}));
console.log("call revoked", kind(() => (fnRev.proxy as any)()));
console.log("construct revoked", kind(() => new (fnRev.proxy as any)()));
console.log("arrow not constructor", kind(() => Reflect.construct(new Proxy(() => 1, {}) as any, [])));
console.log("array proxy", typeof new Proxy([], {}));
