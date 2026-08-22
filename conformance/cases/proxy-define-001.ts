// §10.5.6 [[DefineOwnProperty]] — and the §10.1.9.2 step 2.e path
// that reaches it from an ordinary [[Set]] with a proxy receiver.
const t: any = {};
const log: string[] = [];
const p: any = new Proxy(t, {
  defineProperty(target: any, key: any, desc: any) {
    log.push("dp:" + String(key) + ":" + String(desc.value) + ":" + String(desc.writable));
    return Reflect.defineProperty(target, key, desc);
  },
});

Object.defineProperty(p, "a", { value: 1, writable: true, enumerable: true, configurable: true });
console.log(t.a);
console.log(Reflect.defineProperty(p, "b", { value: 2, configurable: true }));
console.log(t.b);
console.log(log.join(" | "));

// A refusing trap.
const no: any = new Proxy({}, { defineProperty() { return false; } });
console.log(Reflect.defineProperty(no, "x", { value: 1 }));
try {
  Object.defineProperty(no, "x", { value: 1 });
} catch (e: any) { console.log("refused:", e instanceof TypeError); }

// No trap — forwards to the target.
const q: any = new Proxy({}, {});
Object.defineProperty(q, "k", { value: 9, enumerable: true, configurable: true });
console.log(Object.getOwnPropertyDescriptor(q, "k").value);

// §10.1.9.2 step 2.e — an ordinary [[Set]] whose RECEIVER is a proxy
// ends in the receiver's defineProperty, not its set. Reflect.set
// inside a set trap must therefore terminate.
const dst: any = {};
const seen: string[] = [];
const recvProxy: any = new Proxy(dst, {
  set(target: any, key: any, v: any, r: any) {
    seen.push("set:" + String(key));
    return Reflect.set(target, key, v, r);
  },
  defineProperty(target: any, key: any, d: any) {
    seen.push("dp:" + String(key));
    return Reflect.defineProperty(target, key, d);
  },
});
recvProxy.z = 5;
console.log(seen.join(","), dst.z);
