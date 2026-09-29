function trial(i: number): number {
  try {
    throw i
  } catch (e) {
    return e as number
  }
}

const n = parseInt(process.argv[2], 10)
let total = 0
for (let i = 0; i < n; i = i + 1) {
  total = total + trial(i)
}
console.log(total)
