let total = 0
const n = parseInt(process.argv[2], 10)
const input = process.argv[3]
const sep = process.argv[4]
for (let i = 0; i < n; i = i + 1) {
  const parts = input.split(sep)
  total = total + parts.length
}
console.log(total)
