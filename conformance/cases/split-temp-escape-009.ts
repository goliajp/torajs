// split products minted inside a map callback, read as a 2-d table
function table(text: string): string[][] {
  return text.split(';').map((l) => l.split(','))
}
function widths(t: string[][]): string {
  let out = ''
  for (let i = 0; i < t.length; i = i + 1) out = out + t[i].length.toString() + t[i][0]
  return out
}
const text = 'a1,b1,c1;a2,b2;a3,b3,c3,d3' + (10).toString()
const rows = text.split(';').map((l) => l.split(','))
console.log(rows.length, rows[0].length, rows[2].length)
console.log(rows[0][1], rows[1][1], rows[2][3])
for (const row of rows) console.log(row.join('-'))
for (let i = 0; i < rows.length; i = i + 1) for (let j = 0; j < rows[i].length; j = j + 1) console.log(rows[i][j])
const ann: string[][] = text.split(';').map((l) => l.split(','))
ann[1].push('p' + ann.length.toString())
console.log(ann[1].join('-'), ann[1][2])
const t = table(text)
console.log(widths(t), t[2].join(''))
console.log(widths(text.split(';').map((l) => l.split(','))))
const flat = text.split(';').flatMap((l) => l.split(','))
console.log(flat.join(' '), flat.length, flat[4])
const lens = text.split(';').map((l) => l.split(',').length)
console.log(lens.join(','))
const firsts = text.split(';').map((l) => l.split(',')[0])
console.log(firsts.join(','))
const sorted = text.split(';').map((l) => l.split(',').reverse())
console.log(sorted[2].join(','), sorted[0][0])
const filtered = text.split(';').map((l) => l.split(',').filter((c) => c.length > 1))
console.log(filtered[1].join(','), filtered.length)
