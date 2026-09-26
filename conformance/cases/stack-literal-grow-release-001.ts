// A non-escaping array literal lives in its function's frame; growing
// it past its literal length (an out-of-range index write, a length
// write) or punching a hole moves part of it to the heap. Each call and
// each loop iteration starts from a fresh literal again.
function grow(k: number): string {
  const s: number[] = [1, 2, 3, 4]
  s[k] = 9
  return s.length + ":" + s[k] + ":" + (5 in s) + ":" + s[3]
}
for (let i = 0; i < 3; i = i + 1) console.log(grow(6 + i))
function lengthen(n: number): string {
  const s: number[] = [7, 8]
  s.length = n
  s[n - 1] = 5
  return s.length + ":" + s[0] + ":" + (2 in s) + ":" + s[n - 1]
}
for (let i = 0; i < 3; i = i + 1) console.log(lengthen(40 + i))
function hole(i: number): string {
  const s: number[] = [1, 2, 3, 4]
  delete s[i]
  return s.length + ":" + (i in s) + ":" + s[(i + 1) & 3]
}
for (let i = 0; i < 4; i = i + 1) console.log(hole(i))
let acc = 0
for (let i = 0; i < 4; i = i + 1) {
  const s: number[] = [0, 0, 0, 0]
  s[20 + i] = i
  acc = acc + s.length + s[20 + i]
}
console.log(acc)
