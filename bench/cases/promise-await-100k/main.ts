let total = 0
for (let i = 0; i < 100000; i = i + 1) {
  total = total + (await Promise.resolve(i))
}
console.log(total)
