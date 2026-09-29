let total = 0
const n = parseInt(process.argv[2], 10)
for (let i = 0; i < n; i = i + 1) {
  const parts = "3 4 + 2 * 5 +".split(" ")
  total = total + parts.length
}
console.log(total)
