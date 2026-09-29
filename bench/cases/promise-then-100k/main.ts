function add1(v: number): number { return v + 1 }

const n = parseInt(process.argv[2], 10)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  total = total + (await Promise.resolve(i).then(add1))
}
console.log(total)
