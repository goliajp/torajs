// A this-using named function cast to `any` is a receiver-first value
// wherever the cast sits (RFC 20260915 item 3): the any world's call
// paths honor FLAG_CLOSURE_RECV_FIRST, so `new` on `(F as any).bind(..)`
// — the bind kernel's bound cell constructing its target with a fresh
// `this`, the bound this playing no part (§10.4.1.2) — runs F over the
// fresh object, a call over the bound this, `.call` on the bound
// function cannot re-bind, and `name` / `length` are the bound
// function's. The plain forwarder used to pin `this` to undefined here
// ("cannot assign to a property of this any value").
function F(this: any, a: number) { this.a = a; }
const FB = (F as any).bind(null);
const o = new FB(5);
console.log("n1", o.a, typeof FB, FB.name, FB.length);
const o1 = new ((F as any).bind({ z: 1 }))(6);
console.log("n2", o1.a, o1.z);
function G(this: any, a: number, b: number) { this.s = a + b; return this; }
const G1 = (G as any).bind({ k: 1 }, 2);
console.log("n3", G1(3).s, G1(3).k, G1.name, G1.length);
const o2 = new G1(4);
console.log("n4", o2.s, o2.k, Object.keys(o2).join(","));
console.log("n5", G1.call({ k: 9 }, 5).k, G1.apply({ k: 9 }, [6]).s);
const G2 = G1.bind({ k: 7 }, 10);
console.log("n6", G2().s, G2().k, G2.name, G2.length, new G2().s);
function P(this: any) { this.made = true; return 42; }
const p = new ((P as any).bind(undefined))();
console.log("n7", p.made, typeof p);
const call1 = (G as any).call({ k: 2 }, 1, 1);
console.log("n8", call1.s, call1.k, (G as any).apply({ k: 3 }, [2, 2]).k);
