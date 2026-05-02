const q: number[] = []
let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  q.push(i)
  if (q.length > 16) {
    total = total + q.shift()!
  }
}
while (q.length > 0) {
  total = total + q.shift()!
}
console.log(total)
