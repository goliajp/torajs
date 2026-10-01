// a split product as an array literal element
function rows(a: string, b: string): string[][] {
  return [a.split(','), b.split(';')]
}
const a = 'p,q,r' + (2).toString()
const b = 'x;y' + (3).toString()
const lit: string[][] = [a.split(','), b.split(';'), ['own' + a]]
console.log(lit.length, lit[0].length, lit[1].length)
console.log(lit[0][2], lit[1][1], lit[2][0])
for (const row of lit) console.log(row.join('~'))
lit[1].push('pushed' + lit.length.toString())
console.log(lit[1].join('~'), lit[1][2])
const r = rows(a, b)
console.log(r[0].join('.'), r[1].join('.'), r[0][0], r[1][1])
const inferred = [a.split(','), b.split(';')]
console.log(inferred[0][1], inferred[1][0], inferred[0].length)
for (const row of inferred) for (const cell of row) console.log(cell)
