// the single-allocation regexp replace copies short gaps and
// replacements without a memcpy; every piece length from 0 to past 16
// must land byte-exact

const re = /a(b+)c/g
let pad = ''
for (let n = 0; n < 20; n = n + 1) {
  const s = pad + 'abc' + pad + 'abbbc' + pad
  const r1 = s.replace(re, 'XY')
  const r2 = s.replace(re, pad + '#')
  console.log(n, r1.length, r1, r2.length, r2)
  pad = pad + String.fromCharCode(48 + (n % 10))
}

// replacement lengths 1..16 and 17+
let rep = ''
for (let n = 1; n < 22; n = n + 1) {
  rep = rep + String.fromCharCode(97 + (n % 26))
  console.log('abc-abbc-'.replace(re, rep))
}

// no match, non-global, `$&` (general path), and matches at both ends
console.log('xyz'.replace(re, 'Q'), 'xyz'.replace(re, 'Q').length)
console.log('abc abc'.replace(/a(b+)c/, '[$&]'))
console.log('abc abbc'.replace(re, '<$&>'))
console.log('abcabbc'.replace(re, ''))
console.log('abbbbbbbbbbbbbbbbbbbbc'.replace(re, 'z'))
