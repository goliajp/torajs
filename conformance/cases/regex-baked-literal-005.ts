// Literals written with an astral character outside u mode: the build
// step splits the character into its two code units before compiling,
// so the non-u literal matches units and the u literal a code point.

const a = /^[😀]$/
const b = /^[😀]$/u
console.log(a.test("😀"), b.test("😀"))
const m = /😀+/.exec("x😀😀y")
console.log(m === null ? null : m[0].length, m === null ? null : m.index)
const c = /^.$/
const d = /^.$/u
console.log(c.test("😀"), d.test("😀"))
