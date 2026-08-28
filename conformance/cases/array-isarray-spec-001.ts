// §22.1.2.2 Array.isArray IS §7.2.2 IsArray — the same predicate
// `Object.prototype.toString` step 3 asks. It walks a Proxy to its
// target and throws on a revoked one. (The arguments half of the
// spec text is a recorded gap: 517-06.)
console.log(Array.isArray([1]), Array.isArray({}), Array.isArray(null), Array.isArray(5))

console.log(Array.isArray(new Proxy([1], {})))
console.log(Array.isArray(new Proxy({}, {})))
console.log(Array.isArray(new Proxy(new Proxy([1], {}), {})))

// The arguments answer is a recorded gap (517-06) and lives in
// `object-proto-tostring-arguments-001`; what matters here is that a
// split result is not mistaken for one.
const parts: any = "a-b".split("-")
console.log(Array.isArray(parts), Array.isArray("xy".split("")))

const r = Proxy.revocable([1], {})
r.revoke()
try { console.log(Array.isArray(r.proxy)) }
catch (e) { console.log("threw", (e as any).constructor.name) }

// The pending throw must reach a catch even when the answer is read.
const r2 = Proxy.revocable([1], {})
r2.revoke()
try { const v: any = r2.proxy; console.log("unreachable", Array.isArray(v)) }
catch (e) { console.log("threw", (e as any).constructor.name) }
