// elements of an escaped split product outlive the parent string
function collect(n: number): string[][] {
  const out: string[][] = []
  for (let i = 0; i < n; i = i + 1) {
    const line = 'row' + i.toString() + ',val' + (i * 7).toString() + ',end'
    out.push(line.split(','))
  }
  return out
}
function hold(n: number): { parts: string[] }[] {
  const out: { parts: string[] }[] = []
  for (let i = 0; i < n; i = i + 1) {
    let line = 'h' + i.toString() + ';' + (i * 3).toString()
    out.push({ parts: line.split(';') })
    line = 'overwritten' + i.toString()
  }
  return out
}
function churn(n: number): number {
  let total = 0
  for (let i = 0; i < n; i = i + 1) {
    const junk = 'junk' + i.toString() + 'junkjunkjunk' + (i * 13).toString()
    total = total + junk.length
  }
  return total
}
const table = collect(50)
const held = hold(50)
console.log(churn(2000))
let sum = 0
for (let i = 0; i < table.length; i = i + 1) sum = sum + table[i][0].length + table[i][1].length + table[i][2].length
console.log(sum, table[0].join('|'), table[49].join('|'), table[25][1])
let hs = ''
for (const h of held) hs = hs + h.parts[1]
console.log(hs, held[49].parts.join('/'), held[0].parts.length)
let keep: string[] = []
for (let i = 0; i < 20; i = i + 1) {
  const s = 'k' + i.toString() + ' ' + (i * i).toString()
  keep = s.split(' ')
}
console.log(churn(500), keep.join('='), keep[1], keep.length)
