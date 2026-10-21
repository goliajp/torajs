// RegExps stored in a typed array reach the any world when the array does
const rs: RegExp[] = [/x+/, /y/i]
const a: any = rs
console.log(a[0].test('xx'), a.length, a[1].flags)
console.log(a.map((r: any) => r.source).join(','))
console.log(rs.length)
