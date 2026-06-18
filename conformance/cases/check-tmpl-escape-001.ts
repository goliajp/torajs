// Template literal escape sequences per ES §12.8.6.2.
// Pre-fix tora preserved escapes literally in template lit segments
// while strings (`"\n"`) correctly mapped to control bytes. This
// fixture exercises the corrected lexer path: `\n / \t / \r / \\ /
// \` / \$ / \" / \'`. Interpolation slots stay unchanged (`${...}`
// is its own escape, handled outside the new arm).

// case A: newline + tab
console.log(`line1\nline2`)
console.log(`x\ty`)

// case B: carriage return + null + form feed + backspace
console.log(`a\rb`.length)
console.log(`x\0y`.length)
console.log(`p\fq`.length)
console.log(`m\bn`.length)

// case C: backslash + backtick (template-only)
console.log(`a\\b`)
console.log(`pre\`post`)

// case D: $ escape (prevents interp when followed by `{`)
console.log(`pre\${1+1}post`)

// case E: interpolation still works around escapes
const n = 42
console.log(`val=${n}\nend`)

// case F: quotes inside template literal (passthrough, harmless)
console.log(`"hello"`)
console.log(`it's`)

// case G: regression — no-escape segments unchanged
console.log(`plain`)
console.log(`a${1}b`)
