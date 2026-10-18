// performance.now() is milliseconds since the time origin on a clock that
// never goes back; performance.timeOrigin is the origin as a wall-clock
// time, so their sum stays near Date.now().
const t0 = performance.now()
let s = 0
for (let i = 0; i < 100000; i++) s += i % 7
const t1 = performance.now()
console.log(typeof t0, typeof performance.timeOrigin, s)
console.log(t0 >= 0, t1 >= t0, t0 < 60000)
console.log(Math.abs(performance.timeOrigin + performance.now() - Date.now()) < 1000)
console.log(performance.timeOrigin <= Date.now(), performance.timeOrigin > 1.6e12)
function elapsed(f: () => void): number {
  const a = performance.now()
  f()
  return performance.now() - a
}
const d = elapsed(() => {
  let x = 0
  for (let i = 0; i < 1000; i++) x += i
})
console.log(d >= 0, d < 10000)
let last = performance.now()
let ok = true
for (let i = 0; i < 1000; i++) {
  const n = performance.now()
  if (n < last) ok = false
  last = n
}
console.log(ok)
console.log(typeof performance, typeof process, typeof Bun)
