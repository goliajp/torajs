// V3-18 wedge — TS optional field syntax `field?: T` in `type`
// alias bodies and inline-obj type annotations. Per TS spec §3.9,
// optional fields permit absence; subset models this as `T | null`
// since we don't yet carry a separate Type::Undefined for property
// absence. Pre-fix tora's parser bailed with 'expected `:` after
// field name, got Question'.
//
// Subset limitation: Nullable<primitive> (number / boolean) still
// reads back as 0 / false instead of null — that requires real
// undefined substrate (Phase D). The wedge here only unlocks the
// reference-typed case (string, arrays, class instances), which
// is the dominant use site in real TS code.

type O = { a: number; b?: string }
let o: O = { a: 1, b: null }
console.log(o.a)                       // 1
console.log(o.b)                       // null
console.log(o.b === null)              // true

let o2: O = { a: 2, b: "hello" }
console.log(o2.a)                      // 2
console.log(o2.b)                      // hello
console.log(o2.b === null)             // false

// Inline-obj annotation (no `type` alias).
let p: { name?: string; arr?: number[] } = { name: null, arr: [1, 2, 3] }
console.log(p.name)                    // null
console.log(p.arr)                     // [ 1, 2, 3 ]

// Multiple optional fields.
type Person = { id: number; bio?: string; tags?: string[] }
let q: Person = { id: 7, bio: "engineer", tags: ["ts", "rust"] }
console.log(q.id)                      // 7
console.log(q.bio)                     // engineer
console.log(q.tags)                    // [ 'ts', 'rust' ]
