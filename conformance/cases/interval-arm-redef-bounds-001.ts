// A compare bounds an index on one edge, then a definition on some path
// to the read (an if arm, the loop's own latch) moves it out of range:
// the read may not take the bound, and an out-of-range read gives
// undefined.
const n = Number(process.argv[2] ?? '10')

function tenSquares(): number[] {
  const arr: number[] = []
  for (let k = 0; k < 10; k++) arr.push(k * k)
  return arr
}

function armRedef(): string {
  const arr = tenSquares()
  let d = 0
  let out = ''
  while (d < 10) {
    if (d === 5) d = 1000
    out += arr[d] + ' '
    d++
  }
  return out
}

// the bound comes from an if around the loop; the loop body moves it
function outerBound(): string {
  const arr = tenSquares()
  let d = (n / 5) | 0
  let out = ''
  if (d < 5) {
    for (let t = 0; t < 4; t++) {
      out += arr[d] + ' '
      d = d * 7
    }
  }
  return out
}

// an arm moving the index below zero
function armNegative(): string {
  const arr = tenSquares()
  let d = 0
  let out = ''
  while (d < 8) {
    if (d === 3) d = -100
    out += arr[d] + ' '
    d = d < 0 ? 4 : d + 1
  }
  return out
}

// a write through the index past the bound must not land in memory
function armWrite(): string {
  const arr = tenSquares()
  let d = 0
  while (d < 10) {
    if (d === 7) d = 12
    arr[d] = -1
    d++
  }
  return arr.length + ' ' + arr.join(',')
}

console.log(armRedef())
console.log(outerBound())
console.log(armNegative())
console.log(armWrite())
