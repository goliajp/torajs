// An else-if chain that tests one value against constants and picks a
// constant per case compiles to compares and selects instead of
// branches. Every case and the default must still answer what the
// branches answered: for a chain longer than the select cap, a chain
// whose middle test reads another value, picks that are not
// constants, a repeated case value, string picks, and a float value
// that no integer case matches.
function kindOf(c: number): number {
  let kind = 6
  if (c === 43) kind = 2
  else if (c === 45) kind = 3
  else if (c === 42) kind = 4
  else if (c === 40) kind = 5
  return kind
}

function longChain(c: number): number {
  let r = -1
  if (c === 0) r = 10
  else if (c === 1) r = 11
  else if (c === 2) r = 12
  else if (c === 3) r = 13
  else if (c === 4) r = 14
  else if (c === 5) r = 15
  else if (c === 6) r = 16
  return r
}

function mixed(c: number, d: number): number {
  let r = 0
  if (c === 1) r = 100
  else if (d === 2) r = 200
  else if (c === 3) r = 300
  else if (c === 4) r = 400
  return r
}

function nonConst(c: number, x: number): number {
  let r = x
  if (c === 1) r = x + 1
  else if (c === 2) r = x * 2
  else if (c === 3) r = 7
  return r
}

function repeated(c: number): number {
  let r = 0
  if (c === 5) r = 1
  else if (c === 5) r = 2
  else if (c === 6) r = 3
  return r
}

function names(c: number): string {
  let s = 'other'
  if (c === 1) s = 'one'
  else if (c === 2) s = 'two'
  else if (c === 3) s = 'three'
  return s
}

const src = '+-*() a+b-(c*d) 7'
let kinds = ''
let sum = 0
for (let i = 0; i < src.length; i = i + 1) {
  const c = src.charCodeAt(i)
  kinds = kinds + kindOf(c)
  sum = sum + longChain(c - 40) + mixed(c - 39, i) + nonConst(c - 39, i) + repeated(c - 36)
}
console.log(kinds, sum)

const out: number[] = []
for (let c = -1; c < 9; c = c + 1) {
  out.push(longChain(c), mixed(c, 2), mixed(c, c), nonConst(c, 10), repeated(c))
}
console.log(out.join(','))

const names4: string[] = []
for (let c = 0; c < 5; c = c + 1) names4.push(names(c))
console.log(names4.join(' '))

console.log(kindOf(43.5), kindOf(NaN), kindOf(-0), longChain(-0), longChain(2.5), longChain(1e300))
