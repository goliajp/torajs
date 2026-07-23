// ES §22.2.1.1 Static Semantics — CharacterClassRange Early Errors:
// (1) `[z-a]` reversed range is SyntaxError in every mode (u, v, non-u).
// (2) Under u/v, a shorthand ClassEscape (`\d` `\D` `\w` `\W` `\s`
// `\S` `\p{}`) can't be a range endpoint. Non-u annexB path lets `-`
// fall through as a literal on either side. v-mode already rejects
// via its independent class-set parser; this fixture drives the u
// and non-u path in `parser/class.rs`.

function tryCompile(pat: string, flags: string = ''): string {
  try {
    new RegExp(pat, flags)
    return 'ok'
  } catch (e: any) {
    if (e instanceof SyntaxError) return 'SyntaxError'
    return 'other:' + e.name
  }
}

// reversed ranges — all modes reject
console.log(tryCompile('[z-a]'))
console.log(tryCompile('[z-a]', 'u'))
console.log(tryCompile('[z-a]', 'v'))
// forward + equal ranges — accept
console.log(tryCompile('[a-z]'))
console.log(tryCompile('[a-a]'))
console.log(tryCompile('[0-9]', 'u'))
console.log(tryCompile('[\\u0041-\\u005A]', 'u'))

// shorthand class as range LHS — u/v reject, non-u annexB accept
console.log(tryCompile('[\\d-a]'))
console.log(tryCompile('[\\d-a]', 'u'))
console.log(tryCompile('[\\d-a]', 'v'))
console.log(tryCompile('[\\w-a]'))
console.log(tryCompile('[\\w-a]', 'u'))

// shorthand class as range RHS — u/v reject, non-u annexB accept
console.log(tryCompile('[a-\\d]'))
console.log(tryCompile('[a-\\d]', 'u'))
console.log(tryCompile('[a-\\d]', 'v'))
console.log(tryCompile('[a-\\s]'))
console.log(tryCompile('[a-\\s]', 'u'))

// both endpoints shorthand — u reject, non-u accept
console.log(tryCompile('[\\d-\\w]'))
console.log(tryCompile('[\\d-\\w]', 'u'))

// property class as endpoint under u — reject
console.log(tryCompile('[\\p{L}-a]', 'u'))
console.log(tryCompile('[a-\\p{L}]', 'u'))

// literal hyphen at end / after `]` guard — still accept
console.log(tryCompile('[a-]'))
console.log(tryCompile('[-a]'))

// actual runtime use of legal patterns — sanity
console.log(/[a-z]/.test('m'))
console.log(/[a-a]/.test('a'))
console.log(/[0-9]/u.test('5'))
