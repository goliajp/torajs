// ES §22.2.1.1 Static Semantics: Early Errors — two GroupNames with
// the same StringValue in the same Alternative is a SyntaxError.
// ES2025 Duplicate Named Capture Groups proposal carves out
// disjunction siblings: `(?<x>a)|(?<x>b)` is legal but
// `(?<x>a)(?<x>b)` is not. All literal-regex cases still parse-OK
// under tr because the literal path is baked (throw-aware runtime
// path is `new RegExp` only, per handoff L3b) — this fixture
// exercises `new RegExp(...)` for the reject face.

function tryCompile(pat: string): string {
  try {
    new RegExp(pat)
    return 'ok'
  } catch (e: any) {
    if (e instanceof SyntaxError) return 'SyntaxError'
    return 'other:' + e.name
  }
}

// concat dup — reject
console.log(tryCompile('(?<x>a)(?<x>b)'))
// three-in-concat dup — reject
console.log(tryCompile('(?<x>a)(?<x>b)(?<x>c)'))
// top-level disjunction siblings — accept
console.log(tryCompile('(?<x>a)|(?<x>b)'))
// grouped disjunction siblings — accept
console.log(tryCompile('(?:(?<x>a)|(?<x>b))'))
// three-way disjunction siblings — accept
console.log(tryCompile('(?<x>a)|(?<x>b)|(?<x>c)'))
// outer concat + inner disjunction with same name in outer — reject
console.log(tryCompile('(?<x>a)(?:(?<x>b)|c)'))
// three-way: two siblings + one outer concat with same name — reject
console.log(tryCompile('((?<x>a)|(?<x>b))(?<x>c)'))
// nested disjunction, both in different top-level branches — accept
console.log(tryCompile('(?<x>a)|(?:(?<x>b)|c)'))
// distinct names in same concat — accept
console.log(tryCompile('(?<x>a)(?<y>b)'))
// same name in different top-level alt branches, one inside nested
// group — accept (both branches mutually exclusive)
console.log(tryCompile('(?:(?<x>a))|(?:(?<x>b))'))

// use — participating side captures, non-participating side
// undefined; §22.2.7.8. Sanity that legal dup still works.
const r = /(?:(?<x>hello)|(?<x>world))/
const m1: any = 'hello there'.match(r)
console.log(m1.groups.x)
const m2: any = 'world of'.match(r)
console.log(m2.groups.x)
