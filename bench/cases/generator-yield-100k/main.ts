function* range(start: number, end: number) {
  let i = start
  while (i < end) {
    yield i
    i = i + 1
  }
}

let total = 0
const passes = 100
for (let p = 0; p < passes; p = p + 1) {
  for (const v of range(0, 1000)) {
    total = total + v
  }
}
console.log(total)
