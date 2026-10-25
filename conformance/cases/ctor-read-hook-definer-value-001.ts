// `__defineGetter__` is inherited by every object, so a program can
// hold it as a value without naming any builtin, store it on
// %Function.prototype% under a method name the dispatch knows, and
// call it there with a run-time key: that defines a `valueOf` getter
// every constructor cell finds (§7.1.1). `+ 1` on the `Object` a
// run-time `constructor` key reads then runs program code with the
// cell as `this`.
const o: any = {}
const def: any = o.__defineGetter__
const h: any = function () {
  return 0
}
const fp: any = h.__proto__
fp.keys = def
const parts: string[] = ["value", "Of"]
const k = parts[0] + parts[1]
let seen: any = null
const holder: any = {}
holder.g = function (this: any) {
  seen = this
  return function () {
    return 1
  }
}
fp.keys(k, holder.g)
const r: Record<string, number> = { a: 1 }
const c: string[] = ["constr", "uctor"]
console.log(r[c[0] + c[1]] + 1)
console.log(seen === null ? "none" : seen.entries(r).length)
