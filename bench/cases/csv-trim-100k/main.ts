function rowLen(line: string): number {
  const parts = line.split(',')
  let total = 0
  for (let i = 0; i < parts.length; i = i + 1) {
    const t = parts[i].trim()
    total = total + t.length
  }
  return total
}

let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  total = total + rowLen('  alpha , beta , gamma , delta , epsilon , zeta  ')
}
console.log(total)
