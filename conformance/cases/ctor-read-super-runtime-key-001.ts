// `super[k]` with a key computed at run time may name `constructor`:
// the home object's [[Prototype]] is %Object.prototype%, whose own
// `constructor` is `Object` (§20.1.3.1). The method hands the cell
// out, and a static is read off it.
const parts: string[] = ["constr", "uctor"]
const k = parts[0] + parts[1]
const o: any = {
  m() {
    return super[k]
  },
}
const d: Record<string, number> = { a: 1, b: 2 }
const C = o.m()
console.log(d[k] === C, typeof C)
console.log(C.keys(d).join(","))
