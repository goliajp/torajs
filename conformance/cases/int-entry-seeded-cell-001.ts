// a number carried round a loop, started from one the program only
// learns at run time: the loop runs on integers when the start is
// one, and the answers are the float ones whatever it is

function lcg(seed: number, trips: number): number {
  let s = seed
  let acc = 0
  for (let i = 0; i < trips; i = i + 1) {
    s = (s * 48271) % 2147483647
    acc = acc + (s % 1000)
  }
  return acc
}
console.log(lcg(Number('42'), 1000), lcg(Number('1'), 10), lcg(Number('0'), 10))
console.log(lcg(Number('-7'), 10), lcg(Number('2.5'), 10), lcg(Number('x'), 3), lcg(Number('Infinity'), 3))
console.log(lcg(Number('186600000000'), 5), lcg(Number('186700000000'), 5), lcg(Number('9007199254740991'), 5))

function hash(start: number, text: string): number {
  let h = start
  for (let i = 0; i < text.length; i = i + 1) {
    h = (h * 31 + text.charCodeAt(i)) % 1000000007
  }
  return h
}
console.log(hash(Number('7'), 'the quick brown fox'), hash(Number('-0'), 'abc'), hash(Number('1.5'), 'abc'))

function keepsSign(start: number, trips: number): number {
  let z = start
  for (let i = 0; i < trips; i = i + 1) {
    z = z * 1
  }
  return 1 / z
}
console.log(keepsSign(Number('-0'), 4), keepsSign(Number('0'), 4), keepsSign(Number('3'), 4))

function countdown(start: number): number {
  let left = start
  let trips = 0
  while (left > 0) {
    left = left - 3
    trips = trips + 1
    if (trips > 50) break
  }
  return trips * 1000 + left
}
console.log(countdown(Number('10')), countdown(Number('10.5')), countdown(Number('-4')), countdown(Number('x')))
