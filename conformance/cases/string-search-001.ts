// V3-18 wedge — String.prototype.search per JS spec §22.1.3.16
// with a string argument. The spec's full definition coerces
// the arg to a RegExp via @@search, but for a plain string the
// result is exactly indexOf — first match position or -1.
// Pre-fix tora had no String.search registered at all, so
// `s.search(needle)` rejected with 'no member .search on type
// String'. The pattern shows up in code that copies idioms
// from JS-style "find me this string" helpers.
//
// Implementation:
// * check.rs registers String.search alongside indexOf /
//   lastIndexOf with shape (string) -> number.
// * ssa_lower routes search through the existing
//   __torajs_str_index_of helper for both Str and Substr
//   receivers. The RegExp-arg form is a follow-up substrate
//   item alongside the broader Symbol.search dispatch
//   (Symbol.search is already in the symbol-name list at
//   line 132 — this wedge just doesn't wire that path).

// Direct Str receiver — the canonical case.
console.log("hello world".search("world"))      // 6
console.log("hello world".search("hello"))      // 0
console.log("hello world".search("xyz"))        // -1

// Empty needle / receiver edge cases.
console.log("aaa".search(""))                   // 0   empty matches at start
console.log("".search("x"))                     // -1
console.log("".search(""))                      // 0

// First match wins — same semantics as indexOf.
console.log("abcabc".search("c"))               // 2
console.log("abcabc".search("b"))               // 1

// Substr receiver — split returns Substr handles; search
// must work without manual to_owned by the user.
let parts = "alpha,beta,gamma".split(",")
console.log(parts[1].search("e"))               // 1
console.log(parts[2].search("zzz"))             // -1

// In a class method — common idiom.
class Doc {
  text: string
  constructor(t: string) { this.text = t }
  pos(needle: string): number { return this.text.search(needle) }
}
let d = new Doc("Hello, World!")
console.log(d.pos("World"))                     // 7
console.log(d.pos("xxx"))                       // -1

// Chained: search → slice based on the result.
let s = "name: John Doe"
let colonAt = s.search(":")
console.log(colonAt)                            // 4
console.log(s.slice(colonAt + 2))               // John Doe
