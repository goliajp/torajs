// `x.constructor == 1` runs ToPrimitive on the `Object` it reads
// (§7.2.14 step 12, §7.1.1), which calls the `valueOf` it finds on
// %Function.prototype% with `Object` as `this`. The program put its
// own there, which keeps the cell and reads a static off it.
const d: any = { a: 1 }
let seen: any = null
const h: any = function () {
  return 0
}
const fp: any = h.__proto__
fp.valueOf = function (this: any) {
  seen = this
  return 1
}
console.log(d.constructor == 1)
console.log(seen.keys(d).join(","))
