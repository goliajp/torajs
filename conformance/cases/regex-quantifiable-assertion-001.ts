// ES §22.2.1.1 Quantifier / QuantifiableAssertion: under u/v the
// Atom :: Assertion form is not quantifiable — `(?=x)+` / `(?!x)*`
// / `(?<=x)?` / `(?<!x){2}` are SyntaxError. Non-u annexB permits
// the "QuantifiableAssertion" as a legacy carve-out; every major
// browser matches that lenience.

function tryCompile(pat: string, flags: string = ''): string {
  try {
    new RegExp(pat, flags)
    return 'ok'
  } catch (e: any) {
    if (e instanceof SyntaxError) return 'SyntaxError'
    return 'other:' + e.name
  }
}

// lookahead + quantifier
console.log(tryCompile('(?=a)+'))      // ok non-u annexB
console.log(tryCompile('(?=a)+', 'u')) // SE
console.log(tryCompile('(?=a)+', 'v')) // SE
console.log(tryCompile('(?=a)*'))
console.log(tryCompile('(?=a)*', 'u'))
console.log(tryCompile('(?=a)?'))
console.log(tryCompile('(?=a)?', 'u'))
console.log(tryCompile('(?=a){2}'))
console.log(tryCompile('(?=a){2}', 'u'))
// negative lookahead
console.log(tryCompile('(?!a)+'))
console.log(tryCompile('(?!a)+', 'u'))
// lookbehind (both directions)
console.log(tryCompile('(?<=a)+'))
console.log(tryCompile('(?<=a)+', 'u'))
console.log(tryCompile('(?<!a)+'))
console.log(tryCompile('(?<!a)+', 'u'))
// non-quantified assertions — always legal
console.log(tryCompile('(?=a)'))
console.log(tryCompile('(?=a)', 'u'))
console.log(tryCompile('(?<=a)', 'u'))
// runtime use — sanity
console.log(/(?=a)/.test('a'))
console.log(/(?<=a)/.test('a'))
