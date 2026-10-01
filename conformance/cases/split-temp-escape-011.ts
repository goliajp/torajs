// a split product as a temporary in the remaining value positions
class Row {
  cells: string[]
  constructor(cells: string[]) {
    this.cells = cells
  }
  wide(): string {
    return this.cells.join('|') + this.cells.length.toString()
  }
}
class Lazy {
  cells: string[] = 'i,j,k'.split(',')
  more: string[] = ('x' + (11).toString() + ',y').split(',')
}
function one(): number {
  return 1
}
function tmpl(s: string): string {
  return `${s.split(',')}|${s.split(',').length}|${s.split(',')[1]}`
}
const s = 'e,f,g' + (12).toString()
const row = new Row(s.split(','))
row.cells.push('h' + row.cells.length.toString())
console.log(row.wide(), row.cells[3])
const lz = new Lazy()
lz.cells.push('l' + lz.more.length.toString())
console.log(lz.cells.join(''), lz.more.join(''), lz.more[0])
console.log(tmpl(s))
console.log('' + s.split(','), String(s.split(',')), s.split(',').toString())
const m = new Map<string, string[]>()
m.set('a', s.split(','))
m.set('b', s.split('f'))
console.log(m.get('a')!.join('+'), m.get('b')![1], m.get('a')!.length)
const st: string[][] = []
st.unshift(s.split(','))
st[0].push('q' + st.length.toString())
console.log(st[0].join('+'))
const seq = (one(), s.split(','))
console.log(seq.join('_'), seq[2])
const sortedTmp: string[] = s.split(',').sort()
sortedTmp.push('zz' + sortedTmp.length.toString())
console.log(sortedTmp.join(','))
const rev: string[][] = []
rev.push(s.split(',').reverse())
console.log(rev[0].join(','), rev[0][0])
console.log(s.split(',').join('+'), s.split(',').length, s.split(',')[2])
for (const x of s.split(',')) console.log(x)
