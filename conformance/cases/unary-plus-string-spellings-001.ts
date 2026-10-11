// ToNumber of strings: the StringNumericLiteral grammar, not the host
// float parser's. Only the exact-case Infinity forms are numbers.
const xs: any[] = ['42', '-0', '+5', '007', '9007199254740993', '-', '+', '',
  '  12  ', '1E3', '5.', '.5', '0x10', '-0x10', 'Infinity', '-Infinity',
  'INFINITY', 'infinity', 'inf', '-inf', 'nan', 'NaN', '1_000', '1e', '١٢']
for (const s of xs) {
  const n = +s
  console.log(JSON.stringify(s), n, Object.is(n, -0))
}
