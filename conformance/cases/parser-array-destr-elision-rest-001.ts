// V3-18 wedge — array destructuring elision + trailing rest per
// ES spec §13.3.3:
//   let [a, , c] = src        — comma-comma skips one slot, the
//                               next bound name reads from the
//                               following index (not the next-
//                               unconsumed-index of the source).
//   let [head, ...tail] = src — tail picks up src.slice(N) where
//                               N is the number of leading entries
//                               consumed (including elisions).
// Pre-fix tora's parser bailed at the first `,` with 'expected
// identifier in array destructuring, got Comma' for elision and
// 'got DotDotDot' for rest.
//
// Implementation: parser.rs/parse_array_destructuring tracks
// `entries: Vec<Option<String>>` (None = elision) plus an
// optional trailing `rest_name`. Emit pass:
//   * Some(name)  → `let name = src[i]`
//   * None        → no LetDecl emitted; index counter still
//                   advances so the next entry reads from the
//                   right slot.
//   * rest_name   → `let rest = src.slice(N)` reusing the
//                   existing Array.prototype.slice 1-arg path.

// Plain destructuring still works.
let [a, b, c] = [1, 2, 3]
console.log(a, b, c)                    // 1 2 3

// Elision in the middle.
let [x, , z] = [10, 20, 30]
console.log(x, z)                       // 10 30

// Elision at the start.
let [, b1, c1] = [100, 200, 300]
console.log(b1, c1)                     // 200 300

// Double elision.
let [, , last] = [7, 8, 9]
console.log(last)                       // 9

// Trailing rest.
let [head, ...tail] = [1, 2, 3, 4]
console.log(head)                       // 1
console.log(tail)                       // [ 2, 3, 4 ]
console.log(tail.length)                // 3

// Rest of empty tail (source shorter than non-rest entries).
let [m, n, ...empty] = [1, 2]
console.log(m, n)                       // 1 2
console.log(empty)                      // []
console.log(empty.length)               // 0

// Only rest (no leading entries) — picks up everything.
let [...all] = [9, 8, 7]
console.log(all)                        // [ 9, 8, 7 ]
console.log(all.length)                 // 3

// Strings.
let strs = ["a", "b", "c", "d"]
let [s1, ...srest] = strs
console.log(s1)                         // a
console.log(srest)                      // [ "b", "c", "d" ]

// Elision + rest combined.
let [, , ...latetail] = [10, 20, 30, 40]
console.log(latetail)                   // [ 30, 40 ]
console.log(latetail.length)            // 2
