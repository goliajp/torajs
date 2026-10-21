// RegExps in object literals, JSON and string coercion
const o = { re: /q/i, n: 1 }
const ao: any = o
console.log(ao.re.ignoreCase, ao.re.test('Q'))
console.log(JSON.stringify({ r: /x/ }))
console.log(`${/t/g}`, '' + /u/m)
console.log(o.re.test('q'))
