// A sum stepped in the loop test, before the exit compare: it runs once
// more than the body does, so the last step can pass 2^53 and the f64
// sum must round there as JS does.
const off = Number(process.argv[2] ?? '0')

function constStart(): number {
  let s = 9007199254740991 - 2 * 10
  let d = 0
  for (; (s += 2), d < 10; d++) {}
  return s
}

function argStart(n: number): number {
  let s = 9007199254740991 - 2 * n + off
  let d = 0
  for (; (s += 2), d < n; d++) {}
  return s
}

// the same step in a while test
function whileTest(n: number): number {
  let s = 9007199254740991 - 3 * n + off
  let d = 0
  while (((s += 3), d < n)) d++
  return s
}

console.log(constStart() % 2)
console.log(argStart(10) % 2, argStart(10))
console.log(argStart(1000) % 2, argStart(1000))
console.log(whileTest(10) % 2, whileTest(10))
console.log(whileTest(7) % 4, whileTest(7))
