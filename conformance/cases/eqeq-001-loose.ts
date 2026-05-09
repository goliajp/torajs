// V3-18 m3 — `==` / `!=` IsLooselyEqual per spec §7.2.13.
// First wedge: Number / Boolean / Null cross-type pairs.
//   Number == Boolean → coerce Boolean to Number (true→1, false→0)
//   Boolean == Number → same
//   null == null      → true
//   null == anything-else (no undefined yet) → false
// String / BigInt / Object cross-type cases ship in later wedges.
console.log(1 == 1)
console.log(1 == 2)
console.log(1 == true)
console.log(true == 1)
console.log(0 == false)
console.log(false == 0)
console.log(null == null)
console.log(null == 0)
console.log(true == 1.0)
console.log(1 != 2)
console.log(1 != true)
console.log(null != 0)
