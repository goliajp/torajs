// a loop compared against a counter another loop left behind (the
// first loop's count, read but never written by the second): the
// compare reads the count the first loop actually ended on, on
// every way through both
function f(a: number, c: number): number {
  let i = 0
  while (i < a) i = i + 1
  let h = 0
  for (let r = 0; r < c; r = r + 1) { if (i < c) h = h + 1 }
  return h
}
function g(a: number, c: number): number {
  let i = 0
  while (i < a) i = i + 1
  let h = 0
  for (let r = 0; r < c; r = r + 1) { if (r < i) h = h + 1 }
  return h + i
}
console.log(f(Number('3'), Number('5')), f(Number('7'), Number('5')), f(Number('2.5'), Number('4')))
console.log(g(Number('3'), Number('5')), g(Number('7'), Number('5')), g(Number('2.5'), Number('4.5')))
