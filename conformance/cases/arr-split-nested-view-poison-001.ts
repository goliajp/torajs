// Containers of split products whose other uses the view layout
// cannot serve: each one keeps owned elements, and every read below
// must still see the stored text after the heap is reused.
function churn(): void {
  const j: string[] = []
  for (let i = 0; i < 64; i++) j.push('qqqqqqqqqqqq' + i)
}
function width(rs: string[][]): number {
  let n = 0
  for (let i = 0; i < rs.length; i++) n = n + rs[i].length
  return n
}
const lines: string[] = []
for (let i = 0; i < 4; i++) lines.push('a' + i + ',bb' + i + ',ccc' + i)

// handed to `any`
const rowsAny: string[][] = []
for (let i = 0; i < 4; i++) {
  const p1 = lines[i].split(',')
  rowsAny.push(p1)
}
churn()
console.log(rowsAny)

// an owned string pushed into an element
const rowsPush: string[][] = []
for (let i = 0; i < 4; i++) {
  const p2 = lines[i].split(',')
  rowsPush.push(p2)
}
rowsPush[1].push('z' + rowsPush.length)
churn()
const rp = rowsPush[1]
console.log(rp.join('|'), rp.length)

// an element of an element written
const rowsSet: string[][] = []
for (let i = 0; i < 4; i++) {
  const p3 = lines[i].split(',')
  rowsSet[i] = p3
}
rowsSet[2][0] = 'w' + rowsSet.length
churn()
const rs2 = rowsSet[2]
console.log(rs2.join('|'))

// the container's length stored
const rowsLen: string[][] = []
for (let i = 0; i < 4; i++) {
  const p4 = lines[i].split(',')
  rowsLen.push(p4)
}
rowsLen.length = 2
churn()
console.log(rowsLen.length, rowsLen[1].length)

// the whole container passed to a function
const rowsArg: string[][] = []
for (let i = 0; i < 4; i++) {
  const p5 = lines[i].split(',')
  rowsArg.push(p5)
}
churn()
console.log(width(rowsArg))

// iterated with for-of
const rowsFor: string[][] = []
for (let i = 0; i < 4; i++) {
  const p6 = lines[i].split(',')
  rowsFor.push(p6)
}
churn()
for (const r6 of rowsFor) console.log(r6.join('+'))

// an element read through another method
const rowsJoin: string[][] = []
for (let i = 0; i < 4; i++) {
  const p7 = lines[i].split(',')
  rowsJoin.push(p7)
}
churn()
console.log(rowsJoin[3].join('/'), rowsJoin[0][2])

// an element popped off the container
const rowsPop: string[][] = []
for (let i = 0; i < 4; i++) {
  const p8 = lines[i].split(',')
  rowsPop.push(p8)
}
const popped = rowsPop.pop()
churn()
console.log(popped ? popped.join('~') : 'none', rowsPop.length)

// a split stored without a binding
const rowsTemp: string[][] = []
for (let i = 0; i < 4; i++) rowsTemp.push(lines[i].split(','))
churn()
const rt = rowsTemp[2]
console.log(rt.join('#'))

// read from a closure
const rowsClo: string[][] = []
for (let i = 0; i < 4; i++) {
  const p9 = lines[i].split(',')
  rowsClo.push(p9)
}
const lens = [0, 1, 2].map((k) => rowsClo[k].length)
churn()
const rc = rowsClo[0]
console.log(lens.join(','), rc.join(':'))

// serialized
const rowsJson: string[][] = []
for (let i = 0; i < 4; i++) {
  const p10 = lines[i].split(',')
  rowsJson.push(p10)
}
churn()
console.log(JSON.stringify(rowsJson))

// written from a class constructor
const rowsCls: string[][] = []
for (let i = 0; i < 4; i++) {
  const p11 = lines[i].split(',')
  rowsCls.push(p11)
}
class Grow {
  n: number
  constructor() {
    rowsCls[0].push('ctor' + rowsCls.length)
    this.n = 1
  }
}
const g = new Grow()
churn()
const rk = rowsCls[0]
console.log(g.n, rk.join('|'))
