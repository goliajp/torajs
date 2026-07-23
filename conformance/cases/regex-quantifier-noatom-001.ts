// ES §22.2.1.1 Term / Quantifier Early Error: a Quantifier must
// bind to an Atom. `*` `+` `?` at pattern start were already
// rejected; well-formed brace quantifiers (`{n}` / `{n,}` /
// `{n,m}`) with no preceding Atom were previously accepted as a
// literal `{` — bun/JSC reject both in u and non-u. Malformed brace
// bodies (`{}`, `{a}`, `{`) still take the annexB literal-`{` path.

function tryCompile(pat: string, flags: string = ''): string {
  try {
    new RegExp(pat, flags)
    return 'ok'
  } catch (e: any) {
    if (e instanceof SyntaxError) return 'SyntaxError'
    return 'other:' + e.name
  }
}

// well-formed brace quantifier with no preceding Atom — SyntaxError
console.log(tryCompile('{2}'))
console.log(tryCompile('{2}', 'u'))
console.log(tryCompile('{2,}'))
console.log(tryCompile('{2,}', 'u'))
console.log(tryCompile('{2,3}'))
console.log(tryCompile('{2,3}', 'u'))
// After alternation branch — same rule (each Alternative starts
// fresh)
console.log(tryCompile('a|{2,3}'))
console.log(tryCompile('a|{2,3}', 'u'))
// Legal: brace quantifier after an Atom
console.log(tryCompile('a{2,3}'))
console.log(tryCompile('a{2,3}', 'u'))
console.log(tryCompile('(ab){10,5}'.replace('{10,5}', '{2,10}')))
// annexB fallback — malformed brace body reads as literal `{` in
// non-u; u is not affected because the brace never opens a valid
// quantifier
console.log(tryCompile('{'))          // annexB literal
console.log(tryCompile('{}'))         // annexB literal
console.log(tryCompile('{a}'))        // annexB literal
console.log(tryCompile('{,3}'))       // annexB literal
// runtime use — sanity
console.log(/a{2,3}/.test('aaa'))
console.log(/a{2,3}/u.test('aa'))
