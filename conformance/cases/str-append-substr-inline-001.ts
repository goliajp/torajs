// Builder loops appending split parts (views of the line) to one cell:
// every part length the inline append copies (0 to 16 bytes) and the
// first it leaves to the kernel (17), totals that land the cell exactly
// on its room and one past it, a part of the accumulator itself, a
// UTF-16 line, a Latin-1 line past ASCII (the cell loses its ASCII
// mark), and a cell another reference still holds.
const seed = Number(process.argv[2] ?? '7')
const digits = '0123456789abcdefghij'.repeat(seed - 5)

function rebuild(line: string, sep: string): string {
  let out = ''
  const parts = line.split(',')
  for (let k = 0; k < parts.length; k++) {
    out = out + parts[k] + sep
  }
  return out
}

function joined(line: string): string {
  let out = ''
  const parts = line.split(',')
  for (let k = 0; k < parts.length; k++) {
    out = out + parts[k]
  }
  return out
}

// one part of each length 0..18, alone and three to a line
for (let w = 0; w <= 18; w++) {
  const p = digits.slice(w, w + w)
  const line = p + ',' + p + ',' + p
  const r = rebuild(line, ';')
  const j = joined(line)
  console.log(w, r.length, r, j.length, j)
}

// single-byte parts up to and past each room the cell grows through
for (let n = 1; n <= 70; n++) {
  let line = ''
  for (let i = 0; i < n; i++) {
    line = line + (i > 0 ? ',' : '') + digits[i % 20]
  }
  const j = joined(line)
  console.log(n, j.length, j)
}

// a part of the accumulator itself
let self = 'ab,cd,e'
for (const p of self.split(',')) {
  self = self + p
}
console.log(self)

// a UTF-16 line and a Latin-1 one past ASCII
const wide = joined('a中,b,文c,d')
const accented = joined('café,-,naïve,!')
const plain = joined('abc,def')
console.log(wide, wide.length)
console.log(accented, accented.toUpperCase(), accented.length)
console.log(plain.toUpperCase())

// a cell another reference still holds
let shared = ''
const seen: string[] = []
for (const p of 'w,xx,yyy,zzzz'.split(',')) {
  shared = shared + p
  seen.push(shared)
}
console.log(seen.join('|'))
