// a split product pushed as a temporary into a string[][]
function build(lines: string[]): string[][] {
  const out: string[][] = []
  for (let i = 0; i < lines.length; i = i + 1) out.push(lines[i].split(','))
  return out
}
const lines: string[] = []
for (let i = 0; i < 4; i = i + 1) lines.push('a' + i.toString() + ',bb' + i.toString())
const pre: string[][] = []
for (let i = 0; i < 4; i = i + 1) pre.push(lines[i].split(','))
let last: string[] = []
for (let i = 0; i < 4; i = i + 1) {
  const parts = pre[i]
  console.log(parts[0], parts[1], parts.length)
  last = parts
}
console.log(last[last.length - 1])
const built = build(lines)
for (const row of built) {
  console.log(row.join('+'), row.length)
  for (const cell of row) console.log(cell + '!')
}
built[1].push('tail' + built.length.toString())
console.log(built[1].join('|'), built[1][2])
