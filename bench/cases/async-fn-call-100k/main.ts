async function double(v: number): Promise<number> {
  return v + v
}

let total = 0
for (let i = 0; i < 100000; i = i + 1) {
  total = total + (await double(i))
}
console.log(total)
