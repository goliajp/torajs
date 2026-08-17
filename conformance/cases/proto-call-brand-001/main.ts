// rotation 431 — X.prototype.m.call(recv) on the brand-checked
// families (Date / Map / Set / Promise / Function) skips the
// direct-method rewrite and rides the reified proto method cell,
// so a wrong-brand receiver throws the spec's runtime TypeError
// instead of dying on a compile-time member reject; legal
// receivers dispatch bun-equal through the same cell.
const m = new Map([[1, 2]]);
console.log(Map.prototype.has.call(m, 1), Map.prototype.get.call(m, 1));
console.log(Set.prototype.has.call(new Set([2]), 2));
const d = new Date(0);
console.log(Date.prototype.getTime.call(d));
function g(this: any, x: number) { return this.v + x; }
const bound = Function.prototype.bind.call(g, { v: 10 });
console.log(bound(5));
console.log(Function.prototype.call.call(g, { v: 1 }, 2));
try { Map.prototype.entries.call(true); } catch (e) { console.log("m", (e as Error).constructor.name); }
try { Date.prototype.toISOString.call(7); } catch (e) { console.log("d", (e as Error).constructor.name); }
try { Function.prototype.bind.call(undefined); } catch (e) { console.log("f", (e as Error).constructor.name); }
try { Set.prototype.add.call("s", 1); } catch (e) { console.log("s", (e as Error).constructor.name); }
const p = Promise.resolve(3);
Promise.prototype.then.call(p, (v: number) => console.log("p", v));
