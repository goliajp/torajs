// `.` crosses a three-byte character that starts like U+2028 but is not
// a line terminator; `$` under m holds before U+2028 / U+2029.
const wide = (s: string) => s + '\u{1F600}'
for (const c of ['‧', '€', ' ', ' ']) {
  const s = 'a' + c + 'c'
  console.log(/a.c/.test(s), /a.c/.test(wide(s)), s.match(/a.c/) !== null, s.search(/a.c/))
}
console.log('ab cd '.match(/.+$/gm), /a.*$/m.exec('ab cd')?.[0])
console.log('x\nab ab\nab'.match(/(?:^\w+\s*)+/m)?.[0].length, 'a\naab'.split(/a*^b/m))
