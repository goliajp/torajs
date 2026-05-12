// V3-18 P2.4.c.4 — computed property names `{ [<key>]: value }`
// per JS spec §13.2.5. Subset only supports literal-string keys
// at compile time (struct layouts are static); runtime dictionary
// keys defer to the dynamic-substrate phase.
//
// Pre-fix tora's parser hard-rejected with 'expected field name
// in object literal, got LBracket'. Now: when the next token is
// `[`, parse a literal-string key inside, treat the rest of the
// field as if it were a bare-name field.

let o = { ["x"]: 5 }
console.log(o.x)                         // 5

let p = { ["a"]: 1, ["b"]: "hi", ["c"]: true }
console.log(p.a, p.b, p.c)               // 1 hi true

// Mixed with regular fields.
let q = { regular: 100, ["computed"]: 200 }
console.log(q.regular, q.computed)        // 100 200

// Nested computed.
let r = { ["outer"]: { ["inner"]: 42 } }
console.log(r.outer.inner)                // 42

// Shorthand still works (no regression).
let x = 7
let y = 8
let s = { x, y }
console.log(s.x, s.y)                    // 7 8
