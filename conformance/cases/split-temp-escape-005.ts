// a split product returned as a temporary
function cut(s: string): string[] {
  return s.split(',')
}
function cutInferred(s: string) {
  return s.split(',')
}
const cutArrow = (s: string) => s.split(';')
const cutArrowAnn = (s: string): string[] => s.split(';')
function* each(lines: string[]) {
  for (let i = 0; i < lines.length; i = i + 1) yield lines[i].split(',')
}
class Cutter {
  sep: string
  constructor(sep: string) {
    this.sep = sep
  }
  cut(s: string): string[] {
    return s.split(this.sep)
  }
}
const s = 'one,two;three,four' + (5).toString()
const c1 = cut(s)
c1.push('five' + c1.length.toString())
console.log(c1.join('|'), c1[1], c1.length)
const c2 = cutInferred(s)
console.log(c2.join('|'), c2[0], c2.length)
const c3 = cutArrow(s)
console.log(c3.join('|'), c3[1], c3.length)
const c4: string[] = cutArrowAnn(s)
c4.push('six' + c4.length.toString())
console.log(c4.join('|'), c4[2])
for (const row of each([s, 'u,v' + s])) console.log(row.join('#'), row[row.length - 1])
const k = new Cutter('t')
const c5 = k.cut(s)
console.log(c5.join('T'), c5.length)
const all: string[][] = [cut(s), cutInferred(s), cutArrow(s)]
for (const row of all) for (const cell of row) console.log(cell)
