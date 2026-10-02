// A pattern whose DFA outgrows the eager build cap: `(a|b)*a(a|b){N}`
// has about 2^(N+1) states, so the RegExp fills its table while it
// searches. The calls mix test / exec / replace / match / lastIndex on
// the same RegExp objects, the way the lastIndex fixtures do.

function lcg(seed: number, n: number): string {
  let x = seed
  let out = ''
  for (let i = 0; i < n; i++) {
    x = (x * 75 + 74) % 65537
    out += (x >> 3) % 2 === 0 ? 'a' : 'b'
  }
  return out
}

const g = /(a|b)*a(a|b){9}/g
const hay = 'xx' + lcg(7, 40) + 'yy' + lcg(11, 25) + 'zz'
console.log(g.lastIndex)
console.log(g.test(hay))
console.log(g.lastIndex)
console.log(g.test(hay))
console.log(g.lastIndex)
console.log(g.test(hay))
console.log(g.lastIndex)
g.lastIndex = 30
const m = g.exec(hay)
console.log(m === null ? 'null' : m.index + ':' + m[0] + ':' + m[1] + ':' + m[2])
console.log(g.lastIndex)
console.log(hay.replace(g, '<$&>'))
console.log(g.lastIndex)
const all = hay.match(g)
console.log(all === null ? 'null' : all.join(','))

// plain (no g): lastIndex ignored
const plain = /(a|b)*a(a|b){9}/
plain.lastIndex = 5
console.log(plain.test(hay))
console.log(plain.lastIndex)
console.log(plain.test('ab'))
const pm = plain.exec('cc' + lcg(3, 30))
console.log(pm === null ? 'null' : pm.index + ':' + pm[0].length)

// sticky: anchored at lastIndex
const y = /(a|b)*a(a|b){9}/y
console.log(y.test(hay))
console.log(y.lastIndex)
y.lastIndex = 2
console.log(y.test(hay))
console.log(y.lastIndex)

// word boundaries around an over-cap body
const wb = /\b(a|b)*a(a|b){8}\b/g
const words = lcg(5, 12) + ' ' + lcg(9, 14) + ' ' + lcg(13, 9) + ' c' + lcg(17, 20)
console.log(words.match(wb))
console.log(wb.test(words), wb.lastIndex)

// one probe from 0 over a 2^15-state table: `[abc]*` keeps every
// start alive, so the walk goes to the end of the input, and random
// bytes reach a new state almost every step. The cache holds about
// 6700 of these rows: over 9k bytes it clears and carries on; over
// 40k it clears three times, gives up after the accept at 16, and the
// Pike VM finishes the search from 0.
const big = /[abc]*a[ab]{14}c/
const edge = 'a' + 'b'.repeat(14) + 'c'
const mid = edge + lcg(21, 9000) + edge
const long = edge + lcg(23, 40000) + edge
console.log(big.test(mid), big.test(long))
const bm = big.exec(long)
console.log(bm === null ? 'null' : bm.index + ':' + bm[0].length)
console.log(mid.replace(big, 'X'), long.replace(/[abc]*a[ab]{14}c$/, 'Y'))

// property classes: the capped build meets a state the pending class
// cannot serve before it reaches the cap, so the constructor hands the
// program to the Pike VM. Poison met by a lazy fill is covered by the
// unit tests of the lazy table.
const prop = /(a|b)*a(a|b){8}(\p{L}|\p{N})/u
const pe = prop.exec('ab' + lcg(4, 20) + 'é9')
console.log(pe === null ? 'null' : pe.index + ':' + pe.join('|'))
console.log(prop.test(lcg(8, 15) + '7'))
console.log(prop.test('aaaa'))
