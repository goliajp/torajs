// V3-18 m1.h.28 — `console.log(arr_of_substr)` was reading
// parent-pointer bytes as data because Substr layout differs
// from Str:
//   Str:    { hdr@0, len@8, data@16 inline }
//   Substr: { hdr@0, len@8, parent@16, offset@24 }
// Both elements share the slot pointer, but the data lives at
// `parent + 16 + offset` for Substr — the existing arr_print_str
// helper printed garbage.
//
// Fix: separate __torajs_arr_print_substr helper. The dispatch
// table picks by Type::Str vs Type::Substr.

console.log("a-b-c".split("-"))
console.log("hello world".split(" "))
console.log("comma,sep,values".split(","))
console.log("path/to/file.ext".split("/"))
console.log("trailing,".split(","))
console.log("".split(","))
