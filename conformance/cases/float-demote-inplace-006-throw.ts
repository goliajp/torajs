// A throw out of the loop hands the sum to the catch: once while it is
// still an integer, once after it crossed 2^53, and once with no throw.
// A try around the loop keeps the sum f64 today; this pins the answer
// for the day it demotes.
function low(stop: number): number {
  const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
  const prefix = '  Hello42 word  '
  const ring: string[] = ['', '', '', '', '', '', '', '']
  const n = parseInt(process.argv.length > 99 ? '3' : '4000', 10)
  let total = 0
  try {
    for (let i = 0; i < n; i = i + 1) {
      const s = prefix + tail
      ring[i & 7] = s
      total = total + s.charCodeAt(i & 15)
      if (i === stop) throw new Error('stop ' + s.length)
    }
  } catch (e) {
    return total + 0.5
  }
  return total
}
function high(stop: number): number {
  const tail = String(process.argv.length > 99 ? 'zz' : 'xy')
  const prefix = '  Hello42 word  '
  const ring: string[] = ['', '', '', '', '', '', '', '']
  const n = parseInt(process.argv.length > 99 ? '3' : '4000', 10)
  let total = 9007199254740991 - 2000
  try {
    for (let i = 0; i < n; i = i + 1) {
      const s = prefix + tail
      ring[i & 7] = s
      total = total + s.charCodeAt(i & 15)
      if (i === stop) throw new Error('stop ' + s.length)
    }
  } catch (e) {
    return total - 9007199254740991
  }
  return total
}
console.log(low(100), low(9999), high(10), high(3000), high(9999))
