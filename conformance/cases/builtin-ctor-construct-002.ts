// [[Construct]] through a builtin-constructor VALUE, the collection
// and weak families: Map / Set / WeakMap / WeakSet seed from the
// optional iterable (§24.1.1.1 steps 5–9 — an abrupt seed is the
// caller's catchable throw), WeakRef checks its target (§26.1.1.1).
// Symbol and BigInt throw on any `new` (§20.4.1 / §21.2.1 step 1:
// NewTarget is not undefined), not a silent primitive.
const M: any = Map;
const m = new M([["a", 1], ["b", 2]]);
console.log(m.size, m.get("b"), new M().size, new M(null).size);
const S: any = Set;
const s = new S("hello");
console.log(s.size, s.has("l"), new S(undefined).size);
const WM: any = WeakMap;
const k1 = {}, k2 = {};
const wm = new WM([[k1, "one"]]);
console.log(wm.get(k1), wm.has(k2), new WM().has(k1));
const WS: any = WeakSet;
const ws = new WS([k1]);
console.log(ws.has(k1), ws.has(k2), new WS().has(k1));
const WR: any = WeakRef;
console.log(new WR(k1).deref() === k1);
try { new WR(1); console.log("no throw") } catch (e: any) { console.log("threw", e.constructor.name) }
try { new M([1]); console.log("no throw") } catch (e: any) { console.log("threw", e.constructor.name) }
try { new WS([1]); console.log("no throw") } catch (e: any) { console.log("threw", e.constructor.name) }
const Sym: any = Symbol, Big: any = BigInt;
try { new Sym("x"); console.log("no throw") } catch (e: any) { console.log("threw", e.constructor.name) }
try { new Big(1); console.log("no throw") } catch (e: any) { console.log("threw", e.constructor.name) }
console.log(Reflect.construct(M, [[["z", 26]]]).get("z"));
console.log("done");
