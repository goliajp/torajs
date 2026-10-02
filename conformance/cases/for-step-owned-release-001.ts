// A `for` step clause follows the expression-statement contract: an
// owned temp it produces (a call answering an object, an array or a
// string) is released at the end of the step, and an Ident-target
// assignment there mints no consumer stake. Every binding touched by
// a step is read back after a 200k-iteration run.
function mkObj(i: number): { i: number } { return { i: i } }
function mkArr(i: number): number[] { return [i, i + 1] }
function mkStr(i: number): string { return "s" + i }

let n = 0
for (let i = 0; i < 200000; mkObj(i)) {
  i = i + 1
  n = n + 1
}
for (let i = 0; i < 200000; mkArr(i)) {
  i = i + 1
  n = n + 1
}
for (let i = 0; i < 200000; mkStr(i)) {
  i = i + 1
  n = n + 1
}
console.log(n)

let last: { i: number } = mkObj(-1)
for (let i = 0; i < 200000; last = mkObj(i)) {
  i = i + 1
}
console.log(last.i)

let tail = ""
for (let i = 0; i < 1000; tail = tail + mkStr(i).length) {
  i = i + 1
}
console.log(tail.length)

let picked: number[] = mkArr(0)
for (let i = 0; i < 5; picked = mkArr(i)) {
  i = i + 1
}
console.log(picked[0], picked[1])
