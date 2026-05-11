// V3-18 m2.f — `<NamespaceCtor>.prototype.<method>.call(recv, ...)`
// AST-level rewrite to direct `recv.<method>(...)`. Tora has no
// real prototype object so the literal traversal would fail at
// `Number.prototype.toString` (Type::Null doesn't have .toString).
//
// AST pre-pass `desugar_prototype_call` rewrites the shape so
// downstream check.rs / ssa_lower see only the direct method call.
// Common test262 idiom for testing methods on borrowed receivers
// without auto-boxing.

console.log(Number.prototype.toString.call(5))           // "5"
console.log(Number.prototype.toString.call(255))         // "255"
console.log(Number.prototype.toString.call(0))           // "0"

console.log(String.prototype.indexOf.call("abc", "b"))   // 1
console.log(String.prototype.toUpperCase.call("hi"))     // HI
console.log(String.prototype.charCodeAt.call("abc", 0))  // 97
console.log(String.prototype.startsWith.call("hello", "he")) // true

// .apply variant not yet supported (needs spread-call substrate).
