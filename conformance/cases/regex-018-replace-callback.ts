// P9.5-A1 — String.prototype.replace(re, fn) / replaceAll(re, fn)
// callback form per ES spec §22.1.3.18 / §22.1.3.19.
//
// A1 scope: callback shape is `(m: string) => string` only.
// Capture-group spread / offset+input args are A1.1 (multi-arg
// callbacks are rejected at compile time, not silent-wrong).

// 1. Basic — single-match, no flags. cb maps the matched bytes.
console.log("abcd".replace(/b./, (m: string): string => m.toUpperCase()))

// 2. Global flag (`g`) — cb fires per match, all replaced.
console.log("a1b2c3".replace(/\d/g, (m: string): string => `[${m}]`))

// 3. replaceAll with regex — implicit global iteration through cb.
console.log("a-b-c".replaceAll(/-/g, (m: string): string => "+"))

// 4. Sticky (`y`) + fn — P9.4-A1.1 sticky semantics must hold through
//    the fn path. /a/gy on "aXab" stops after the first miss, not
//    "find next a elsewhere" — so the second `a` is NOT replaced.
console.log("aXab".replace(/a/gy, (m: string): string => "Y"))

// 5. Sticky + replaceAll + fn.
console.log("xxyy".replaceAll(/x/gy, (m: string): string => "Z"))

// 6. Empty-match handling — `b*` matches an empty span before each
//    char + at end. cb fires for each empty match; advance-on-empty
//    must not infinite-loop.
console.log("ab".replace(/b*/g, (m: string): string => `<${m}>`))

// 7. Replacement that returns the input verbatim — identity cb.
console.log("hello".replace(/l/g, (m: string): string => m))

// 8. Replacement larger than the match — cb expansion.
console.log("a1b2".replace(/\d/g, (m: string): string => m + m))

// 9. No match — cb never fires; input returned unchanged.
console.log("abc".replace(/z/g, (m: string): string => "X"))

// 10. Closure capture — cb captures an outer counter binding.
let counter = 0
console.log(
    "aaa".replace(/a/g, (m: string): string => {
        counter = counter + 1
        return String(counter)
    }),
)
console.log(counter)

// 11. cb that returns empty string — equivalent to deleting matches.
console.log("a1b2c3".replace(/\d/g, (m: string): string => ""))

// 12. Anchored pattern (^) — only one match.
console.log("foo".replace(/^./g, (m: string): string => "X"))

// 13. Existing Str-repl path must not regress.
console.log("abcd".replace(/b./, "X"))
console.log("a1b2".replace(/\d/g, "$&!"))
console.log("foo".replaceAll(/o/g, "0"))

// 14. cb returning a constant — no use of match arg.
console.log("a-b-c".replaceAll(/-/g, (_m: string): string => "/"))

// 15. Multi-char match — cb sees the full slice, not just first char.
console.log("ab12cd34".replace(/\d+/g, (m: string): string => `<${m}>`))
