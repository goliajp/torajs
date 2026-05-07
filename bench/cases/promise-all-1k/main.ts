let arr: Promise<number>[] = []
for (let i = 0; i < 1000; i = i + 1) {
  arr.push(Promise.resolve(i))
}
let r: number[] = await Promise.all(arr)
let total = 0
for (let i = 0; i < r.length; i = i + 1) {
  total = total + r[i]
}
console.log(total)
