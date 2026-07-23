// ES §22.2.1.1 DecimalEscape Early Error: under u/v the `\0` NUL
// escape must not be followed by a decimal digit. `\01` looks like
// legacy octal to non-u annexB but is a SyntaxError in strict u/v.

function tryCompile(pat: string, flags: string): string {
  try {
    new RegExp(pat, flags)
    return 'ok'
  } catch (e: any) {
    if (e instanceof SyntaxError) return 'SyntaxError'
    return 'other:' + e.name
  }
}

// `\0` alone — NUL escape, legal in every mode
console.log(tryCompile('\\0', ''))
console.log(tryCompile('\\0', 'u'))
console.log(tryCompile('\\0', 'v'))
// `\0` followed by digit — SyntaxError under u/v, annexB legal in non-u
console.log(tryCompile('\\01', ''))
console.log(tryCompile('\\01', 'u'))
console.log(tryCompile('\\01', 'v'))
console.log(tryCompile('\\09', ''))
console.log(tryCompile('\\09', 'u'))
console.log(tryCompile('\\00', ''))
console.log(tryCompile('\\00', 'u'))
// `\0` followed by non-digit — legal in every mode (NUL then that char)
console.log(tryCompile('\\0a', 'u'))
console.log(tryCompile('\\0\\n', 'u'))
// runtime use — sanity
console.log(/\0/.test('\0'))
console.log(/\0/u.test('\0'))
