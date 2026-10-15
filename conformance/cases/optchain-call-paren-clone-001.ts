// parentheses end an optional chain in a cloned body too: the all-any
// twin of a generator keeps `(o?.m).call(o)` a TypeError when o is null
function* gen(s: string, o: any) {
  yield s.length
  yield (o?.m).call(o)
}
const obj = { k: 7, m(this: any) { return this.k } }
for (const v of gen("ab", obj)) console.log(v)
const g: any = gen
for (const v of g(5, obj)) console.log(v)
try {
  for (const v of g("xyz", null)) console.log(v)
} catch (e) {
  console.log("threw", e instanceof TypeError)
}
try {
  for (const v of gen("q", undefined)) console.log(v)
} catch (e) {
  console.log("threw", e instanceof TypeError)
}
