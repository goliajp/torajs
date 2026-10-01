// a call that answers a bound split product's own array, escaping
function tails(lines: string[]): string[][] {
  const out: string[][] = []
  for (let i = 0; i < lines.length; i = i + 1) {
    const parts = lines[i].split(',')
    out.push(parts.reverse())
    console.log(parts[0] + ' ' + parts.length.toString() + ' ' + parts.join('<'))
  }
  return out
}
function sortedOf(line: string): string[] {
  const parts = line.split(',')
  const first = parts[0]
  const r: string[][] = [parts.sort()]
  console.log(first + ' ' + parts[0] + ' ' + parts[parts.length - 1])
  return r[0]
}
const lines: string[] = []
for (let i = 0; i < 3; i = i + 1) lines.push('x' + i.toString() + ',y' + i.toString() + ',z' + i.toString())
const t = tails(lines)
for (const row of t) console.log(row[0], row[2], row.join('>'), row.length)
t[0].push('w' + t.length.toString())
console.log(t[0].join('>'), t[0][3])
const so = sortedOf('q,c,m' + (13).toString() + ',a')
so.push('zz' + so.length.toString())
console.log(so.join(' '), so[0], so[4])
const direct: string[][] = []
direct.push(lines[1].split(',').reverse())
direct.push(lines[2].split(',').sort())
console.log(direct[0][0], direct[1][2], direct[0].join(''), direct[1].join(''))
