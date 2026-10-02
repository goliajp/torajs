function next(s: number): number {
  const t = (s * 48271) % 2147483647
  return t === 0 ? 1 : t
}

const n = parseInt(process.argv[2], 10)
const fields = parseInt(process.argv[3], 10)
let seed = parseInt(process.argv[4], 10)

const lines: string[] = []
for (let i = 0; i < 1024; i = i + 1) {
  let line = ''
  for (let f = 0; f < fields; f = f + 1) {
    seed = next(seed)
    if (f > 0) line = line + ','
    line = line + (seed % 100000).toString()
  }
  lines.push(line)
}

const rows: string[][] = []
for (let i = 0; i < 1024; i = i + 1) {
  rows.push([])
}

let total = 0
for (let i = 0; i < n; i = i + 1) {
  const parts = lines[i & 1023].split(',')
  let rebuilt = ''
  for (let k = 0; k < parts.length; k = k + 1) {
    const p = parts[k]
    total = total + p.charCodeAt(p.length - 1)
    rebuilt = rebuilt + p + ';'
  }
  total = total + rebuilt.length
  rows[i & 1023] = parts
}

let held = 0
for (let j = 0; j < rows.length; j = j + 1) {
  const row = rows[j]
  held = held + row.length + row[row.length - 1].length
}
console.log(total)
console.log(held)
