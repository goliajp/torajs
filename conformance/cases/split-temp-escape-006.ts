// a split product assigned to an existing binding, field or element
class Holder {
  items: string[] = []
  set(line: string): void {
    this.items = line.split(',')
  }
}
function run(line: string): string {
  let xs: string[] = []
  xs = line.split(',')
  xs.push('end' + xs.length.toString())
  let ys: string[] = ['seed' + line]
  for (let i = 0; i < 3; i = i + 1) ys = (line + i.toString()).split('b')
  return xs.join('+') + ' ' + ys.join('-') + ' ' + ys[1]
}
const line = 'ab,cb,db' + (6).toString()
let top: string[] = []
top = line.split(',')
console.log(top.join(':'), top[2], top.length)
top.push('t' + top.length.toString())
console.log(top.join(':'))
for (const t of top) console.log(t)
console.log(run(line))
const h = new Holder()
h.set(line)
h.items.push('h' + h.items.length.toString())
console.log(h.items.join('/'), h.items[0], h.items.length)
const grid: string[][] = [[], []]
grid[0] = line.split(',')
grid[1] = line.split('b')
console.log(grid[0].join('.'), grid[1].join('.'), grid[1][3], grid[0].length)
const bag: { parts: string[] } = { parts: [] }
bag.parts = line.split('c')
console.log(bag.parts.join('C'), bag.parts[1])
