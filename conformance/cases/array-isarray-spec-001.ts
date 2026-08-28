// §22.1.2.2 Array.isArray IS §7.2.2 IsArray — the same predicate
// `Object.prototype.toString` step 3 asks. It walks a Proxy to its
// target, throws on a revoked one, and says no to an arguments
// object (an ordinary object with a [[ParameterMap]], not an Array
// exotic one) even though tr mints that as an Arr cell.
console.log(Array.isArray([1]), Array.isArray({}), Array.isArray(null), Array.isArray(5))

console.log(Array.isArray(new Proxy([1], {})))
console.log(Array.isArray(new Proxy({}, {})))
console.log(Array.isArray(new Proxy(new Proxy([1], {}), {})))

// Both halves of the arguments judgment: the static local and the
// same cell seen through `any`.
function f() {
  console.log(Array.isArray(arguments))
  const a: any = arguments
  console.log(Array.isArray(a))
  // ... and the badge walk must agree with it.
  console.log(Object.prototype.toString.call(arguments))
}
f(1, 2)

const r = Proxy.revocable([1], {})
r.revoke()
try { console.log(Array.isArray(r.proxy)) }
catch (e) { console.log("threw", (e as any).constructor.name) }

// The pending throw must reach a catch even when the answer is read.
const r2 = Proxy.revocable([1], {})
r2.revoke()
try { const v: any = r2.proxy; console.log("unreachable", Array.isArray(v)) }
catch (e) { console.log("threw", (e as any).constructor.name) }
