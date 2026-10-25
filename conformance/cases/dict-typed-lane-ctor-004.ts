// A run-time key may name `constructor`, and the dictionary read then
// answers `Object`. `""+` and `+ 1` run ToPrimitive on it (§7.1.1),
// which calls the `valueOf` it finds on %Function.prototype% with
// `Object` as `this`; the program put its own there, which keeps the
// cell and reads statics off it. No literal spells `constructor` and
// nothing names a constructor or a namespace.
const d: Record<string, number> = { a: 1 }
const parts: string[] = ["constr", "uctor"]
const k = parts[0] + parts[1]
let seen: any = null
const h: any = function () {
  return 0
}
const fp: any = h.__proto__
fp.valueOf = function (this: any) {
  seen = this
  return 1
}
console.log("" + d[k])
console.log(seen.keys(d).join(","))
seen = null
console.log(d[k] + 1)
console.log(seen.entries(d).length)
