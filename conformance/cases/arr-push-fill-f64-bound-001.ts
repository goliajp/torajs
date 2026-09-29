// a fill loop whose bound is only known at run time, and is an f64:
// the trip count is the header's, whatever the bound turns out to be

function fill(n: number): number[] {
  const xs: number[] = []
  for (let i = 0; i < n; i = i + 1) {
    xs.push(i)
  }
  return xs
}

function show(label: string, xs: number[]): void {
  console.log(label, xs.length, xs.length > 0 ? xs[xs.length - 1] : -1)
}

show('int', fill(parseInt('10', 10)))
show('fraction', fill(Number('2.5')))
show('half', fill(Number('0.5')))
show('nan', fill(Number('x')))
show('negzero', fill(Number('-0')))
show('negative', fill(Number('-3')))
show('quotient', fill(parseInt('5', 10) / 2))
show('large', fill(parseInt('100000', 10)))

function fillTwice(n: number): number[] {
  const xs: number[] = []
  for (let i = 0; i < n; i = i + 1) {
    xs.push(i)
    xs.push(i * 2)
  }
  return xs
}
show('twice', fillTwice(Number('2.5')))

function fillWhile(n: number): number[] {
  const xs: number[] = []
  let i = 0
  while (i < n) {
    xs.push(i)
    i = i + 1
  }
  return xs
}
show('while', fillWhile(Number('2.5')))

function fillAfter(n: number): number[] {
  const xs: number[] = [7, 8, 9, 10]
  for (let i = 0; i < n; i = i + 1) {
    xs.push(i)
  }
  return xs
}
show('after', fillAfter(parseInt('10', 10)))

let calls = 0
function bump(i: number): number {
  calls = calls + 1
  return i + calls
}
function fillCalling(n: number): number[] {
  const xs: number[] = []
  for (let i = 0; i < n; i = i + 1) {
    xs.push(bump(i))
  }
  return xs
}
show('calling', fillCalling(Number('2.5')))
console.log(calls)

const top = Number('6.25')
const ys: number[] = []
for (let i = 0; i < top; i++) {
  ys.push(i * i)
}
show('top', ys)
