// A dynamic-key read on a `Record<string, number>` answers what
// §10.1.8.1 [[Get]] answers, whatever the declared value type says:
// an inherited Object.prototype member, an accessor on the prototype,
// a value an `any` alias stored, a non-writable entry, a deleted
// entry, a double, -0 and keys outside Latin-1.
const names: string[] = ["toString", "__proto__", "hasOwnProperty", "a", "zz", "ro", "键", "😀", "f"]
const d: Record<string, number> = {}
console.log(typeof d[names[0]], d[names[1]] === Object.prototype, typeof d[names[2]])
console.log(d[names[3]], d[names[3]] ?? 0, (d[names[3]] ?? 0) + 1)

const alias: any = d
alias[names[3]] = "s"
console.log(d[names[3]], d[names[3]] ?? 0, (d[names[3]] ?? 0) + 1)
alias[names[3]] = { n: 1 }
console.log(typeof d[names[3]], JSON.stringify(d[names[3]]))
alias[names[3]] = null
console.log(d[names[3]], d[names[3]] ?? 7)
d[names[3]] = 3
console.log(d[names[3]] + 1, names[3] in d, Object.prototype.hasOwnProperty.call(d, names[3]))

Object.defineProperty(Object.prototype, "zz", {
  get() {
    return 42
  },
  configurable: true,
})
console.log(d[names[4]], (d[names[4]] ?? 0) + 1, names[4] in d)
delete (Object.prototype as any).zz
console.log(d[names[4]], d[names[4]] ?? -1)

Object.defineProperty(d, names[5], { value: 5, writable: false, enumerable: true, configurable: true })
try {
  d[names[5]] = 6
  console.log("wrote")
} catch (e) {
  console.log(e instanceof TypeError)
}
console.log(d[names[5]], (d[names[5]] ?? 0) + 1)

delete d[names[3]]
console.log(d[names[3]], (d[names[3]] ?? 0) + 1, names[3] in d)

d[names[6]] = 1
d[names[7]] = 2
for (let i = 0; i < 5; i = i + 1) {
  d[names[6 + (i & 1)]] = (d[names[6 + (i & 1)]] ?? 0) + 10
}
console.log(d[names[6]], d[names[7]], Object.keys(d).join(","))

d[names[8]] = 1.5
console.log(d[names[8]] + 1, (d[names[8]] ?? 0) * 2)
d[names[8]] = -0
console.log(Object.is(d[names[8]], -0), 1 / d[names[8]])
d[names[8]] = 2147483647
console.log(d[names[8]] + 1, (d[names[8]] ?? 0) + 1)
d[names[8]] = NaN
console.log(d[names[8]], (d[names[8]] ?? 1) === (d[names[8]] ?? 1))

function sum(r: Record<string, number>, ks: string[]): number {
  let s = 0
  for (const k of ks) s = s + (r[k] ?? 100)
  return s
}
function make(): Record<string, number> {
  const out: Record<string, number> = {}
  out[names[3]] = 4
  return out
}
console.log(sum(d, names), sum(make(), [names[3], names[0].slice(0, 2)]), make()[names[3]])
const frozen: Record<string, number> = Object.freeze({ a: 8 })
console.log(frozen[names[3]], frozen[names[0]] === Object.prototype.toString)
const fake: Record<string, number> = new Proxy({} as Record<string, number>, {
  get: (_t, k) => (typeof k === "string" ? k.length : 0),
})
console.log(fake[names[2]], fake[names[6]])
