const xs: number[] = []
const n = parseInt(process.argv[2], 10)
for (let i = 0; i < n; i = i + 1) xs.push(i)
let total = 0
while (xs.length > 0) {
  total = total + xs.pop()!
}
console.log(total)
