// An integer counter squared past 2^53 and past 2^63: Number
// multiplication is f64, so the product rounds instead of staying exact
// or wrapping.
const span = Number(process.argv[2] ?? '6')

function squaresPast63(): string {
  let out = ''
  for (let d = 3037000496; d <= 3037000496 + span; d++) {
    const q = d * d
    out += q + ' '
  }
  return out
}

function squaresPast53(): string {
  let out = ''
  for (let d = 94906262; d <= 94906262 + span; d++) {
    const q = d * d
    out += q + ' ' + (q % 2) + ' '
  }
  return out
}

// the square only tested, never printed
function squareTest(): number {
  let hits = 0
  for (let d = 3037000496; d <= 3037000496 + span; d++) {
    if (d * d > 9223372036854775807) hits++
  }
  return hits
}

// a counter from 1 whose square bounds the loop
function squareBound(n: number): number {
  let d = 1
  while (d * d <= n) d++
  return d
}

console.log(squaresPast63())
console.log(squaresPast53())
console.log(squareTest())
console.log(squareBound(9007199254740993 + span))
