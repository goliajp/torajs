// A typed array assigned into an `any[]` slot: the block is shared as
// it is and the slot's readers rebox each element, the same crossing
// a declaration `let a: any[] = typed` has always admitted.
let a: any[] = []
const s: string[] = ['x', 'y']
a = s
console.log(a.length, a[1], typeof a[0], JSON.stringify(a))
const n: number[] = [1, 2.5]
let b: any[] = []
b = n
console.log(b.length, b[1], typeof b[0], b.join('|'))
const flags: boolean[] = [true, false]
b = flags
console.log(b.length, b[0], typeof b[1])
// a promoted global slot takes the same crossing
let g: any[] = []
function peekG(): string { return g.join('+') + '/' + g.length }
g = s
console.log(peekG(), typeof g[0])
g = n
console.log(peekG(), g[1] + 1)
// the source keeps its own typed reads after the share
s.push('z')
console.log(s.length, s[2], n[0] + n[1])
