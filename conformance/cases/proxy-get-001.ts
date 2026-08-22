// §10.5.8 [[Get]] — the `get` trap, and forwarding when there is none.
const target: any = { a: 1, b: 2 };
const log: any[] = [];

const p: any = new Proxy(target, {
  get(t: any, key: any, recv: any) {
    log.push(String(key));
    return t[key] === undefined ? "miss:" + String(key) : t[key] * 10;
  },
});
console.log(p.a, p.b, p.zzz);
console.log(log.join(","));

// The trap sees the proxy itself as the receiver.
const q: any = new Proxy(target, {
  get(t: any, key: any, recv: any) {
    return recv === q;
  },
});
console.log(q.anything);

// No trap at all — every read forwards to the target.
const r: any = new Proxy(target, {});
console.log(r.a, r.b, r.zzz);

// A handler whose `get` is explicitly undefined also forwards.
const s: any = new Proxy(target, { get: undefined });
console.log(s.a);

// The target keeps working on its own.
console.log(target.a);

// Non-object target / handler is a TypeError (§10.5.14).
try { new Proxy(1 as any, {}); } catch (e: any) { console.log(e instanceof TypeError, "target"); }
try { new Proxy({}, "x" as any); } catch (e: any) { console.log(e instanceof TypeError, "handler"); }

// A proxy over a proxy forwards one hop at a time.
const inner: any = new Proxy({ v: 7 }, { get(t: any, k: any) { return "inner:" + String(k); } });
const outer: any = new Proxy(inner, {});
console.log(outer.v);

// Symbol keys reach the trap too.
const sym = Symbol("s");
const withSym: any = new Proxy({}, { get(t: any, k: any) { return typeof k; } });
console.log(withSym[sym]);
