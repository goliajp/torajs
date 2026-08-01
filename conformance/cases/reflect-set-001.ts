// §28.1.13 Reflect.set(target, key, value) — boolean answer
const o: any = {};
console.log(Reflect.set(o, "a", 1));
console.log(o.a);
// readonly refusal answers false, no throw
const ro: any = {};
Object.defineProperty(ro, "x", { value: 1, writable: false });
console.log(Reflect.set(ro, "x", 2));
console.log(ro.x);
// non-extensible fresh key answers false
const ne: any = { k: 1 };
Object.preventExtensions(ne);
console.log(Reflect.set(ne, "fresh", 2));
console.log(Reflect.set(ne, "k", 3));
console.log(ne.k);
// setter runs (true), getter-only refuses (false)
const acc: any = {};
let captured = 0;
Object.defineProperty(acc, "s", { set(v: any) { captured = v; } });
console.log(Reflect.set(acc, "s", 42), captured);
Object.defineProperty(acc, "g", { get() { return 7; } });
console.log(Reflect.set(acc, "g", 1));
// array element + length
const arr: any = [1, 2, 3];
console.log(Reflect.set(arr, "1", 99));
console.log(arr[1]);
console.log(Reflect.set(arr, "length", 1));
console.log(arr.length);
// primitive target throws
try { Reflect.set(1 as any, "k", 2); } catch (e) { console.log("primitive-throw"); }
// key coercion + poisoned key propagates
const keyobj: any = { toString() { throw new Error("boom"); } };
try { Reflect.set(o, keyobj, 1); } catch (e: any) { console.log("key-throw", e.message); }
// detached call
const rs: any = Reflect.set;
const d: any = {};
console.log(rs(d, "z", 5), d.z);
// reflection face
console.log(Reflect.set.length, Reflect.set.name);
