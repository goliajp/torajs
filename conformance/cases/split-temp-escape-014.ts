// a split product with a limit, bound and escaping
function head(line: string): string[] {
  return line.split(',', 2)
}
function take(xs: string[]): string {
  return xs.join('/') + xs.length.toString()
}
const line = 'a,b,c,d,e' + (14).toString()
const two = line.split(',', 2)
console.log(two.length, two[0], two[1], two.join('+'))
two.push('p' + two.length.toString())
console.log(two.join('+'))
const h = head(line)
h.push('q' + h.length.toString())
console.log(h.join('-'), h[2])
console.log(take(line.split(',', 3)), take(line.split(',', 0)), take(line.split(',', 99)))
const grid: string[][] = []
for (let i = 0; i < 3; i = i + 1) grid.push((line + i.toString()).split(',', i + 1))
for (const row of grid) console.log(row.join('|'), row.length)
const u = line.split(undefined, 1)
console.log(u.length, u[0])
const uz: string[][] = [line.split(undefined, 0), line.split(undefined, 1)]
console.log(uz[0].length, uz[1].length, uz[1][0])
let total = 0
for (let i = 0; i < 20000; i = i + 1) {
  const t = line.split(',', 2)
  total = total + t.length + t[1].length
}
console.log(total)
const anyArr: any[] = []
const shared: string[] = line.split(',')
shared.push('x' + line)
anyArr.push(shared)
console.log(anyArr[0][2], anyArr[0].length)
const lim = line.split(',', 2)
const lims: string[][] = [line.split(',', 2), lim]
console.log(lims[0][1], lims[1][0], lims[0].length, lims[1].join('+'))
