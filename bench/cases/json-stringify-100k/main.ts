function makeRecord(i: number) {
  return { id: i, name: 'row', score: i * 7, active: (i & 1) === 0 }
}

let total = 0
const n = 100000
for (let i = 0; i < n; i = i + 1) {
  const r = makeRecord(i)
  const s = JSON.stringify(r)
  total = total + s.length
}
console.log(total)
