// V3-18 m1.c — JS spec §7.2.14 IsLessThan ToNumber coercion
// for `<` / `>` / `<=` / `>=` with Bool / Null operands.
// Mirror of m1.b's arith coercion. String<String comparison
// needs its own runtime codepoint-cmp helper — deferred to m1.d.
console.log(true < 2)
console.log(true > 0)
console.log(false < true)
console.log(false >= true)
console.log(null < 1)
console.log(null > -1)
console.log(true <= 1)
console.log(false >= 0)
console.log(null <= null)
