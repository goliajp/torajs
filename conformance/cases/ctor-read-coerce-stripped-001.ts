// With no write anywhere that could install a ToPrimitive hook, a
// run-time `constructor` read that is only converted or loosely
// compared hands the cell to the builtin `toString` / `valueOf`
// alone, and the namespace statics can go.
const d: Record<string, number> = { a: 1 }
const parts: string[] = ["constr", "uctor"]
const k = parts[0] + parts[1]
console.log("" + d[k])
console.log(d[k] == 1)
console.log(d[k] + 1)
console.log(d.a + 1)
