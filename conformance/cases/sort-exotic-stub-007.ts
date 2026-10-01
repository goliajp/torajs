// the only exotic index here is an accessor defined on a typed cell
const acc: number[] = [3, 1, 2]
let seen = -1
Object.defineProperty(acc, '1', {
  get() {
    return 0
  },
  set(v: number) {
    seen = v
  },
  configurable: true,
})
acc.sort((p, q) => p - q)
console.log(acc[0], acc[1], acc[2], seen)
