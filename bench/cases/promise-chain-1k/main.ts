function add1(v: number): number { return v + 1 }

const n = parseInt(process.argv[2], 10)
let p = Promise.resolve(0)
for (let i = 0; i < n; i = i + 1) {
  p = p.then(add1)
}
console.log(await p)
