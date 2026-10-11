// A callback declaring a rest parameter collects every argument the
// array method passes past its fixed parameters (§10.2.11 / §23.1.3).
function count(...xs: any[]): number {
  return xs.length
}
console.log([7, 8].map(count).join(','))

function head(a: number, ...xs: unknown[]): number {
  return a + xs.length
}
console.log([7, 8].map(head).join(','))

function kinds(...xs: (number | number[])[]): string {
  return xs.map((x) => (Array.isArray(x) ? 'array' : typeof x)).join('/')
}
console.log([7, 8].map(kinds).join(' '))

const odd = (...xs: any[]): boolean => xs[0] % 2 === 1
console.log([7, 8, 9].filter(odd).join(','), [2, 4].some(odd), [1, 3].every(odd))
console.log([2, 5, 6].find(odd), [2, 5, 6].findIndex(odd))

const seen: string[] = []
;[3, 4].forEach((...xs: any[]) => {
  seen.push(xs.slice(0, 2).join(':') + '/' + xs.length)
})
console.log(seen.join(' '))

function add(acc: number, ...xs: any[]): number {
  return acc + xs[0] + xs.length
}
console.log([1, 2, 3].reduce(add, 0), [1, 2, 3].reduceRight(add, 100))

function byValue(...xs: number[]): number {
  return xs[0] - xs[1]
}
console.log([3, 1, 2].sort(byValue).join(','), [9, 7, 8].toSorted(byValue).join(','))

console.log([1, 2].flatMap((...xs: any[]) => [xs[0], xs.length]).join(','))
