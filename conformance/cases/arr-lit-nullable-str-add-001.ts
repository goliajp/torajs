// An array literal whose element is `x + x` over a callback
// parameter that may hold the string[] undefined sentinel: the add
// goes through the any lane (undefined + undefined is NaN), so the
// literal must too, even though the checker types the element string.
const a: string[] = ["a", "b", "c"]
a.length = 5
console.log(a.flatMap((x) => [x + x]).join("|"))
console.log(a.flatMap((x) => ["s", x + x]).join("|"))
console.log(a.map((x) => [x + x, "t"]).join("|"))
const viaLet = a.flatMap((x) => {
  const y = x + x
  return [y, y.length]
})
console.log(viaLet.join("|"))
function pair(x: string | undefined) {
  return [x + x, "z"]
}
console.log(pair("q").join("|"), pair(undefined).join("|"), typeof pair(undefined)[0])
