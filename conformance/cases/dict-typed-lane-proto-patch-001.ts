// a number dictionary aliasing builtin prototypes: the first write of a
// method name creates an own entry the primitive method calls must see,
// and later in-place writes keep them seeing it
const np: Record<string, number> = Number.prototype as any
np["toFixed"] = 1
for (let i = 0; i < 3; i = i + 1) {
  np["toFixed"] = (np["toFixed"] ?? 0) + 1
}
const x: any = 5
try {
  console.log(x.toFixed(2))
} catch (e) {
  console.log("TypeError", e instanceof TypeError)
}
console.log((Number.prototype as any).toFixed, np["toFixed"])

const sp: Record<string, number> = String.prototype as any
sp["trim"] = 0
sp["trim"] = (sp["trim"] ?? 0) + 7
const s: any = "  a  "
try {
  console.log(s.trim())
} catch (e) {
  console.log("TypeError", e instanceof TypeError)
}
console.log(typeof s.trim, s.trim)

const op: Record<string, number> = Object.prototype as any
op["hasOwnProperty"] = 3
op["hasOwnProperty"] = (op["hasOwnProperty"] ?? 0) * 2
const o: any = { a: 1 }
console.log(o.hasOwnProperty, typeof o.hasOwnProperty)
try {
  console.log(o.hasOwnProperty("a"))
} catch (e) {
  console.log("TypeError", e instanceof TypeError)
}
