// RFC 20260720-ctor-static-reflection 刀 6 — Promise.resolve /
// reject as VALUES (ns_static batch 4). Both statics read |this| as
// the species constructor (§27.2.4.7/.6 step 1), so a DETACHED bare
// call throws the bun/JSC TypeError — the reified cell serves the
// reflection surface (name / length / print / gOPD identity), and
// direct member calls keep riding the typed lowering untouched.

const r = Promise.resolve;
const j = Promise.reject;

// ---- reflection ----
console.log(Promise.resolve.name, Promise.resolve.length);  // resolve 1
console.log(Promise.reject.name, Promise.reject.length);    // reject 1
console.log(Promise.resolve);                               // [Function: resolve]
const d = Object.getOwnPropertyDescriptor(Promise, "resolve");
console.log(d !== undefined && d.value === Promise.resolve); // true
console.log(d && (d as any).writable, d && (d as any).enumerable, d && (d as any).configurable); // true false true

// ---- detached bare call: |this| is undefined → TypeError ----
try { r(42); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
try { j("boom"); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }
try { r(); console.log("no throw"); } catch (e) { console.log((e as Error).name, "|", (e as Error).message); }

// ---- direct member calls stay on the typed lane ----
Promise.resolve(5).then((v) => console.log("direct", v));
Promise.reject("nope").catch((e) => console.log("caught", e));
console.log("sync-tail");
