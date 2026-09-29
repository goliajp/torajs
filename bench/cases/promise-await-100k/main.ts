const n = parseInt(process.argv[2], 10)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  total = total + (await Promise.resolve(i))
}
console.log(total)
