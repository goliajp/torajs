// A dictionary made by `Object.create(null)` inherits nothing: every
// key it does not own answers undefined, `constructor` and
// `__proto__` included. The ordinary dictionary beside it answers the
// inherited members, and a static read off its `constructor` is the
// function `Object` owns.
const bare: Record<string, number> = Object.create(null)
const d: Record<string, number> = {}
const ks: string[] = ["constructor", "toString", "hasOwnProperty", "__proto__", "x"]
bare[ks[4]] = 3
d[ks[4]] = 4
let line = ""
for (const k of ks) {
  line = line + k + ":" + typeof bare[k] + "/" + typeof d[k] + " "
}
console.log(line)
const O: any = d[ks[0]]
console.log(O === Object, O.keys(d).join(","), Object.keys(bare).join(","))
const f: any = d[ks[2]]
console.log(f.call(d, "x"), f.call(d, "toString"))
