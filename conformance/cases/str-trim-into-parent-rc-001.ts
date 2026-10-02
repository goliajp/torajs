// trim of a split piece writes a stack view of the piece's parent; the
// parent is counted when it is a heap string and left alone when it is
// a literal, and either way the view must stay readable wherever it goes

function keep(line: string): string[] {
  const out: string[] = []
  const parts = line.split(',')
  for (let i = 0; i < parts.length; i = i + 1) {
    const t = parts[i].trim()
    out.push(t)
  }
  return out
}

function lens(line: string): number {
  const parts = line.split(',')
  let total = 0
  for (let i = 0; i < parts.length; i = i + 1) {
    const t = parts[i].trim()
    total = total + t.length
  }
  return total
}

// literal parent: the trimmed pieces outlive the function that split it
const fromLiteral = keep('  alpha , beta ,gamma,  , delta  ')
console.log(fromLiteral.length, fromLiteral.join('|'))

// heap parent built at run time, dropped before the pieces are read
let built = ''
for (let i = 0; i < 4; i = i + 1) {
  built = built + ' w' + String(i) + ' ,'
}
const held = keep(built)
built = 'x'
console.log(held.length, held.join('|'), built)

// many rounds over both shapes: a miscounted parent shows up as a
// wrong length or a crash long before the loop ends
let sum = 0
for (let r = 0; r < 2000; r = r + 1) {
  sum = sum + lens(' a , bb ,ccc ,  dddd') + lens(' r' + String(r % 10) + ' , s ')
}
console.log(sum)

// a non-Latin-1 parent takes the two-byte scan
const wide = keep(' été , 日本 ,　x　')
console.log(wide.length, wide.join('|'), wide[2].length)
