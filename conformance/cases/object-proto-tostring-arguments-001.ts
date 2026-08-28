// FLAG_ARR_ARGUMENTS shares bit 1 of the header flags with
// FLAG_SPLIT_BLOCK on the SAME `Tag::Arr` — the occupancy map calls
// bit 1 disjoint-by-tag, but `str_split` mints its single-malloc
// block as an array and stamps it there. Any reader that treated the
// bit as an identity called `"a-b".split("-")` an arguments object,
// in both the §20.1.3.6 badge walk and §7.2.2 IsArray.
//
// So a split result must stay an ordinary array, on both faces and
// however it is spelled.
const parts: any = "a-b".split("-")
console.log(Object.prototype.toString.call(parts), Array.isArray(parts))
console.log(Object.prototype.toString.call("xy".split("")), Array.isArray("xy".split("")))
const joined: any = ["pe-r", "fig"].flatMap((s: any) => s.split("-"))
console.log(JSON.stringify(joined), Array.isArray(joined))

// A plain array and a proxy over one are unaffected.
console.log(Object.prototype.toString.call([1, 2]), Array.isArray([1, 2]))
console.log(Array.isArray(new Proxy([1], {})))

// The arguments cell itself still behaves as itself. (Its BADGE and
// its IsArray answer are a recorded gap — 517-06 — because tr has no
// signal that separates it from an array; both faces answer "array",
// and what matters is that they agree with each other.)
function g() {
  const a: any = arguments
  console.log(a.length, a[0], a[1])
}
g(1, 2)
