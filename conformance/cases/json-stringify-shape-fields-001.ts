function rec(i: number, name: string, on: boolean) {
  return { id: i, name: name, score: i * 7 - 50, active: on }
}

function wide(i: number, label: string) {
  return { aVeryLongFieldNameForTesting: i, b: label, c: -i, flag: i > 2 }
}

const n = parseInt(process.argv[2] ?? '6', 10)
const names = ['row', '', 'a"q', 'back\\slash', 'tab\there', 'café', '中文', 'sixteen-chars-ok', 'seventeen-chars-x', 'x']
let total = 0
for (let i = -3; i < n; i = i + 1) {
  for (let k = 0; k < names.length; k = k + 1) {
    const s = JSON.stringify(rec(i * 1000003, names[k], (i & 1) === 0))
    total = total + s.length
    if (i === 1) console.log(s)
  }
  const w = JSON.stringify(wide(i, names[(i + 9) % names.length]))
  console.log(w)
}
console.log(total)
const big = { id: 9007199254740991, name: 'max', score: -9007199254740991, active: true }
console.log(JSON.stringify(big))
