// `>>>` and a bitwise operator with a number side answer a number
// whatever the other operand holds, so a top-level copy read from a
// function body takes a number slot. With an `any` operand the
// operator runs on the boxed value, and the slot must hold the number
// it answers, not the box.

const x: any = 41
const y: any = '7'
const u = x >>> 1
const v = x & 3
const w = y | 0
const s = y << 2
const t = x > 3
const m: any = -1
const big = m >>> 0
function read(): string {
  return [u, v, w, s, t, typeof u, typeof w].join(' ')
}
function readBig(): string {
  return [big, typeof big, big === 4294967295].join(' ')
}
console.log(read())
console.log(readBig())
