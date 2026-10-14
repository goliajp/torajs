// a method written onto a builtin prototype after many prototypes
// were minted is still seen by the native fast paths
const d: Record<string, number> = {}
for (let i = 0; i < 50; i++) d["k" + (i % 7)] = (d["k" + (i % 7)] ?? 0) + 1
console.log(Object.keys(d).length, d.k0, (d as any).missing)
const s: any = "abc"
console.log(s.toUpperCase(), [3, 1, 2].includes(2), (5).toFixed(1))
;(String.prototype as any).toUpperCase = function () { return "patched:" + this }
;(Array.prototype as any).includes = function () { return "arr-patched" }
;(Number.prototype as any).toFixed = function () { return "num-patched" }
console.log(s.toUpperCase(), ([3, 1, 2] as any).includes(2), (5 as any).toFixed(1))
const o: any = Object.prototype
o.extra = 1
console.log(({} as any).extra, d.extra)
