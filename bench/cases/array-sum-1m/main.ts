let xs: number[] = []
let i = 0
while (i < 10000000) {
  xs.push(i)
  i = i + 1
}
let sum = 0
let j = 0
while (j < xs.length) {
  sum = sum + xs[j]
  j = j + 1
}
console.log(sum)
