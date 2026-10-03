// many read-only number strings in one function: past the frame
// budget the rest stay on the heap, and every one still reads right
const re = /^k\d+$/
function many(n: number): number {
  let t = 0
  for (let i = 0; i < n; i = i + 1) {
    if (('k' + i).match(re) !== null) t = t + 1
    if (('ka' + i).length > 2) t = t + 1
    if (('kb' + i).startsWith('kb')) t = t + 1
    if (('kc' + i).endsWith(String(i % 10))) t = t + 1
    if (('kd' + i).includes('d')) t = t + 1
    if (('ke' + i).indexOf('e') === 1) t = t + 1
    if (('kf' + i).charCodeAt(0) === 107) t = t + 1
    if (('kg' + i) === 'kg' + i) t = t + 1
    if (('kh' + i).length > 2) t = t + 1
    if (('ki' + i).length > 2) t = t + 1
    if (('kj' + i).length > 2) t = t + 1
    if (('kk' + i).length > 2) t = t + 1
    if (('kl' + i).length > 2) t = t + 1
    if (('km' + i).length > 2) t = t + 1
    if (('kn' + i).length > 2) t = t + 1
    if (('ko' + i).length > 2) t = t + 1
  }
  return t
}
function depth(d: number): number {
  if (d === 0) return many(3)
  return depth(d - 1) + (('r' + d).length > 1 ? 1 : 0)
}
console.log(many(parseInt(process.argv[2] ?? '50', 10)), depth(200))
