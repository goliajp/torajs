// V3-18 m1.h.1 — JS spec §7.1.2 ToBoolean coercion at every
// condition site (if / while / do-while / for / ternary).
//   Number 0 → false; non-zero → true; NaN → false
//   String ""  → false; non-empty → true
//   null  → false
//   Object/Array/Closure/etc → always true
// Spec-foundational — every untyped JS program has at least one
// truthy `if (x)` shape. ssa_lower coerces the cond through
// coerce_to_bool before the CondBr.
if (1) console.log("a")
if (0) console.log("b")
if ("x") console.log("c")
if ("") console.log("d")
if (null) console.log("e")

let i: number = 3
while (i) { console.log(i); i = i - 1 }

let s: string = "go"
if (s) console.log("nonempty")

let r: number = 5 ? 10 : 20
console.log(r)

for (let j: number = 1; j; j = j - 1) console.log("loop " + j)
