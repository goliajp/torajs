// Parameter defaults run in parameter order, arrows included, once an
// earlier one runs in the body and a later one assigns.
let n = 0
const order: string[] = []
function note(s: string): number {
  order.push(s)
  return 1
}
const f = (a: any = note('a'), b: any = (n = order.push('b'))) => a + b
console.log(f(), n, order.join(','))
let seen: any
const h = function (a: any = ((seen = arguments), note('c')), b: any = (n = order.push('d'))) {
  return a + b
}
console.log(h(), n, seen.length, order.join(','))
