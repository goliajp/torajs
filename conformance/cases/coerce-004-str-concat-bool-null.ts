// V3-18 m1.d — JS spec §7.1.17 ToString coercion for `+` String
// concat with Boolean / Null operands.
//   ToString(true)  → "true"
//   ToString(false) → "false"
//   ToString(null)  → "null"
// Routes through new __torajs_bool_to_str / __torajs_null_to_str
// runtime helpers before the regular str_concat. Matches bun
// byte-for-byte.
console.log("v=" + true)
console.log("v=" + false)
console.log("v=" + null)
console.log(true + " ok")
console.log(null + " end")
console.log("a" + true + "b")
console.log("count: " + 0 + " items, ok=" + true)
