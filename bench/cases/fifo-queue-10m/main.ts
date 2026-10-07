const q: number[] = []
let total = 0
const n = parseInt(process.argv[2], 10)
const cap = parseInt(process.argv[3], 10)
for (let i = 0; i < n; i = i + 1) {
  q.push(i)
  if (q.length > cap) {
    total = total + q.shift()!
  }
}
while (q.length > 0) {
  total = total + q.shift()!
}
console.log(total)
