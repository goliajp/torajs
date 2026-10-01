// a split product read in place: variable separator for-of, length, index, join
function sum(line: string, sep: string): number {
  let n = 0
  for (const x of line.split(sep)) n = n + x.length
  return n
}
function firstLen(line: string, sep: string): number {
  return line.split(sep)[0].length + line.split(sep).length
}
const sep = (13).toString().length > 1 ? ',' : ';'
const line = 'alpha,beta,gamma,delta' + (13).toString()
console.log(sum(line, sep), sum(line, 'a'), firstLen(line, sep))
for (const x of line.split(sep)) console.log(x + '.')
let joined = ''
for (let i = 0; i < 3; i = i + 1) joined = joined + line.split(sep).join('+') + '|'
console.log(joined)
console.log(line.split(sep).length, line.split(sep)[3], line.split(sep).join(''))
line.split(sep)
console.log((line.split(sep) as string[]).length)
