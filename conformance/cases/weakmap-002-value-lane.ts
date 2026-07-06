// RC-4 F2 — the WeakMap kernel value slot is an AnyValue lane:
// primitive values are legal per spec §24.3.3.4 (only KEYS must be
// held weakly). Pre-fix `m.set(obj, 1)` on a typed (let-bound)
// receiver passed the raw i64 through the heap-ptr rc lane —
// rc_inc(1) SIGSEGV (test262 WeakMap ×4 crash family). Key
// classification per ES CanBeHeldWeakly rides the same commit:
// primitives INCLUDING strings throw for set/add and read as
// absent for has/get/delete, in both the typed and any lanes.

let foo = {};
let bar = {};
let m = new WeakMap();
console.log(m.get(foo));
m.set(foo, 1);
console.log(m.get(foo));
console.log(m.has(foo), m.has(bar));
m.set(foo, "str-val");
console.log(m.get(foo));
m.set(bar, foo);
console.log(m.get(bar) === foo);
console.log(m.delete(foo), m.has(foo));
try { m.set(1, 1); } catch (e) { console.log("caught:", e instanceof TypeError); }
try { m.set("s", 1); } catch (e) { console.log("caught-str:", e instanceof TypeError); }
try { m.set(undefined, 1); } catch (e) { console.log("caught-undef:", e instanceof TypeError); }
console.log(m.has(1), m.get(1));
let s = new WeakSet();
try { s.add(false); } catch (e) { console.log("caught-ws:", e instanceof TypeError); }
s.add(foo);
console.log(s.has(foo), s.has(bar));
