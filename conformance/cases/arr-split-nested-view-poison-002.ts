// A `string[][]` container declared in a `for` head, or as an `if`
// branch's single statement, is not one the census walks, so stores of
// split products into it keep owned strings.
const lines: string[] = ['a,b', 'c,d', 'e,f']
for (let rows: string[][] = []; rows.length < 2; ) {
  const parts = lines[rows.length].split(',')
  rows.push(parts)
  console.log(rows[rows.length - 1][0], rows[rows.length - 1].join('|'))
}
const holes: string[][] = []
for (let i = 0; i < 3; i++) holes.push([])
const p2 = lines[2].split(',')
holes[2] = p2
const r = holes[1]
console.log(r.length, holes[2][1], holes.length)
function build(): number {
  const acc: string[][] = []
  for (let i = 0; i < 3; i++) {
    const parts = lines[i].split(',')
    acc.push(parts)
  }
  console.log(acc[2][0], acc[0][1])
  return acc.length
}
console.log('n', build())
