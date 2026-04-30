function trial(i: number): number {
  try {
    throw i
  } catch (e) {
    return e as number
  }
}

let total = 0
for (let i = 0; i < 100000; i = i + 1) {
  total = total + trial(i)
}
console.log(total)
