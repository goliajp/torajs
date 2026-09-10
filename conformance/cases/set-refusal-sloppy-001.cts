// §13.15.2 PutValue in the sloppy goal: a [[Set]] refusal — a
// non-writable property, a non-extensible receiver, a frozen array's
// index, a primitive base — is discarded, and the assignment still
// answers its right-hand side. Strict code turns the same refusal
// into a TypeError (frozen-array-elem-mutators-any-001,
// check-preventextensions-001 and kin); before this, every goal
// threw. The nullish-base TypeError stays in every goal.
const o: any = Object.freeze({ a: 1 });
o.a = 2; o.b = 3;
console.log(o.a, o.b, (o.a = 9), Object.isFrozen(o));
const p: any = Object.preventExtensions({ x: 1 });
p.y = 2; p.x = 5;
console.log(p.x, p.y);
const ro: any = Object.defineProperty({}, "k", { value: 1, writable: false, enumerable: true });
ro.k = 2; ro["k"] = 3;
console.log(ro.k, Object.keys(ro));
const arr: any = Object.freeze([1, 2, 3]);
arr[0] = 9; arr[5] = 1; arr.length = 0;
console.log(arr, arr.length);
const sealed: any = Object.seal([1, 2]);
sealed[0] = 7; sealed[2] = 8;
console.log(sealed);
const s: any = "abc";
s[0] = "z"; s.foo = 1;
console.log(s, s.foo, s[0]);
const n: any = 5;
n.x = 1;
console.log(n.x);
const key: any = "k";
ro[key] = 4;
let i: any = 0;
arr[i] = 4; arr[i]++; arr[i] += 2;
console.log(ro.k, arr[0]);
try { const nul: any = null; nul.x = 1; } catch (e: any) { console.log(e instanceof TypeError); }
try { const und: any = undefined; und[0] = 1; } catch (e: any) { console.log(e instanceof TypeError); }
const prx: any = new Proxy({}, { set() { return false; } });
prx.q = 1; prx[0] = 2;
console.log(prx.q, prx[0]);
