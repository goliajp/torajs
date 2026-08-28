// §20.1.3.6 step 3 runs `IsArray(O)` before the @@toStringTag Get,
// and §7.2.2 walks a Proxy to its target — so a proxy over an array
// is badged "Array", not "Object", and a revoked one throws from
// step 3.a instead of answering at all.
console.log(Object.prototype.toString.call(new Proxy([1, 2], {})))
console.log(Object.prototype.toString.call(new Proxy({}, {})))

// The walk is recursive: a proxy over a proxy over an array is
// still an array as far as IsArray is concerned.
console.log(Object.prototype.toString.call(new Proxy(new Proxy([1], {}), {})))

// A `get` trap must not be consulted — IsArray reads the internal
// slot, not a property.
const trapped: any = new Proxy([1], { get(t: any, k: any) { return t[k] } })
console.log(Object.prototype.toString.call(trapped))

// Revoked, both over an array and over a plain object.
const ra = Proxy.revocable([1], {})
ra.revoke()
try { console.log(Object.prototype.toString.call(ra.proxy)) }
catch (e) { console.log("threw", (e as any).constructor.name) }
const ro = Proxy.revocable({}, {})
ro.revoke()
try { console.log(Object.prototype.toString.call(ro.proxy)) }
catch (e) { console.log("threw", (e as any).constructor.name) }

// NOT covered here: step 16's `@@toStringTag` override seen THROUGH
// a proxy. `symbol_key_pair` does not forward a symbol-keyed read to
// a proxy's target, so putting the tag on Array.prototype and asking
// a proxy over an array still answers the Array builtinTag. Tracked
// as 517-05 — the fix needs the trap-aware [[Get]] that 517-01 is
// also waiting on.
