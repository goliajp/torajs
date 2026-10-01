// a split product destructured, sliced, switched on, typeof'd and awaited
async function later(line: string): Promise<string[]> {
  return line.split(',')
}
async function main(): Promise<void> {
  const line = 'k,l,m,n' + (15).toString()
  const [a, b] = line.split(',')
  console.log(a, b)
  const [c, ...rest] = line.split(',')
  console.log(c, rest.join('+'), rest.length)
  rest.push('r' + rest.length.toString())
  console.log(rest.join('+'))
  const sl: string[][] = []
  sl.push(line.split(',').slice(1))
  sl.push(line.split(',').slice(0, 2))
  console.log(sl[0].join('.'), sl[1].join('.'), sl[0][2], sl[1].length)
  switch (line.split(',').length) {
    case 4:
      console.log('four')
      break
    default:
      console.log('other')
  }
  switch (line.split(',')[2]) {
    case 'm':
      console.log('em')
      break
    default:
      console.log('no')
  }
  console.log(typeof line.split(','), typeof line.split(',')[0])
  const w = await later(line)
  w.push('w' + w.length.toString())
  console.log(w.join('-'), w[1], w.length)
  const v: string[] = await later(line + ',o')
  console.log(v.join('-'), v.length)
  console.log((await later(line)).join('='), (await later(line))[3])
}
main()
