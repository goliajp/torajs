// a split product handed straight to a string[] parameter
function total(xs: string[]): number {
  let n = 0
  for (const x of xs) n = n + x.length
  return n
}
function grow(xs: string[], extra: string): string {
  xs.push(extra + extra)
  return xs.join('/')
}
function first(xs: string[]): string {
  return xs[0] + ':' + xs.length.toString()
}
function keep(xs: string[]): string[] {
  return xs
}
const src = 'alpha,beta,gamma' + (1).toString()
console.log(total(src.split(',')))
console.log(grow(src.split(','), 'z'))
console.log(first(src.split('a')))
const kept = keep(src.split(','))
kept.push('w' + kept.length.toString())
console.log(kept.join('-'), kept[3], kept.length)
const viaArrow = (xs: string[]): string => xs[xs.length - 1]
console.log(viaArrow(src.split(',')))
console.log(JSON.stringify(src.split(',')))
console.log(src.split(','))
