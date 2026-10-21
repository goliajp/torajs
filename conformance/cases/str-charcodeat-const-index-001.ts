// charCodeAt at constant indexes: inside, at and past the end, negative,
// and around the 4096-unit reach of an immediate-offset load (index +
// 16 bytes of header for a byte, + 8 halfwords for a UTF-16 unit), on
// Latin-1 and UTF-16 strings.
function codes(s: string): string {
  return [
    s.charCodeAt(0),
    s.charCodeAt(1),
    s.charCodeAt(2),
    s.charCodeAt(-1),
    s.charCodeAt(4079),
    s.charCodeAt(4080),
    s.charCodeAt(4087),
    s.charCodeAt(4088),
    s.charCodeAt(4095),
    s.charCodeAt(4096),
    s.charCodeAt(4103),
  ].join(' ')
}

for (const s of ['', 'a', 'ab', 'é', '中文', 'xyz']) {
  console.log(s.length, codes(s))
}
const narrow = 'q'.repeat(4096) + 'Z' + 'r'.repeat(7)
const wide = '中'.repeat(4096) + 'Z' + '文'.repeat(7)
console.log(narrow.length, codes(narrow))
console.log(wide.length, codes(wide))
let total = 0
for (const p of 'alpha,é,中文,,z'.split(',')) {
  total = total + p.length + (p.length > 0 ? p.charCodeAt(0) : 0)
}
console.log(total)
