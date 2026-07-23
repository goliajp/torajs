// ES §22.2.3.1 RegExp Runtime Semantics: the flag string is
// validated at construction time — duplicate letters and unknown
// letters are Early Errors → SyntaxError. `u`+`v` combined is
// already rejected via a separate `flag_conflict` check.

function tryCompile(pat: string, flags: string): string {
  try {
    new RegExp(pat, flags)
    return 'ok'
  } catch (e: any) {
    if (e instanceof SyntaxError) return 'SyntaxError'
    return 'other:' + e.name
  }
}

// duplicate flags — SyntaxError
console.log(tryCompile('a', 'gg'))
console.log(tryCompile('a', 'ii'))
console.log(tryCompile('a', 'gigi'))
console.log(tryCompile('a', 'igmi'))
// unknown letters — SyntaxError
console.log(tryCompile('a', 'z'))
console.log(tryCompile('a', 'gz'))
console.log(tryCompile('a', 'i1'))
console.log(tryCompile('a', 'g,'))
console.log(tryCompile('a', 'gA'))
// u + v combined — SyntaxError (pre-existing coverage — sanity)
console.log(tryCompile('a', 'uv'))
console.log(tryCompile('a', 'vu'))
// valid flag combos — accept
console.log(tryCompile('a', ''))
console.log(tryCompile('a', 'g'))
console.log(tryCompile('a', 'gi'))
console.log(tryCompile('a', 'gimsy'))
console.log(tryCompile('a', 'du'))
console.log(tryCompile('a', 'dv'))
console.log(tryCompile('a', 'digmsuy'))
// runtime use — sanity that valid flags still work
console.log(/a/g.test('aaa'))
console.log(/A/i.test('a'))
