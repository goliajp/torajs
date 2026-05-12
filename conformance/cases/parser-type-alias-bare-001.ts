// V3-18 wedge — TS bare type alias `type X = T` where T is
// a non-struct type-ann (primitive, array-of, nullable union,
// fn-type, etc). Per TS spec §3.10.1 type aliases are name
// references for any inhabitable type, not just struct
// shapes. Pre-fix tora's parser bailed with 'expected `{` to
// begin type body, got Ident("number")' since parse_type_decl
// only accepted the brace-delimited struct form.
//
// Implementation: parse_type_decl now accepts a non-LBrace
// next-token after `=` and parses a bare type-ann, encoding
// the result as Stmt::TypeDecl { fields = [("__alias__",
// "<ann>")] } — a sentinel field name that check.rs and
// ssa_lower detect to register the underlying Type without
// wrapping in a Struct.

type ID = number
function getID(): ID { return 42 }
console.log(getID())                   // 42

type Name = string
let n: Name = "alice"
console.log(n)                          // alice

type IntArr = number[]
let arr: IntArr = [1, 2, 3]
console.log(arr.length)                // 3
console.log(arr[1])                    // 2

type MaybeStr = string | null
let m: MaybeStr = null
console.log(m)                          // null
m = "hi"
console.log(m)                          // hi

// Alias chains (alias of alias).
type IntID = number
type UserID = IntID
let uid: UserID = 1234
console.log(uid)                       // 1234

// Array-of-alias.
type Name2 = string
let names: Name2[] = ["a", "b", "c"]
console.log(names.join(","))           // a,b,c

// Function-type alias.
type Cb = (x: number) => number
let f: Cb = (x: number) => x * 2
console.log(f(7))                      // 14

// Generic bare alias — `type P<T> = T[]`.
type List<T> = T[]
let xs: List<number> = [1, 2, 3]
console.log(xs.length)                 // 3
let ss: List<string> = ["x", "y"]
console.log(ss.join("-"))              // x-y
