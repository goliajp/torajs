// three loops with run-time bounds in a chain: the second reads what
// the first left, the third what the second left. Each loop reads
// the merged value, whichever of the three is the larger.

function spaces(src: string): number {
  let n = 0
  let i = 0
  while (i < src.length) {
    if (src.charCodeAt(i) === 32) n = n + 1
    i = i + 1
  }
  return n
}

// read inside the loop bodies, the reading loops the larger
function bodyReads(a: number, b: number, c: number): string {
  let s = ''
  for (let i = 0; i < a; i = i + 1) s = s + 'ab c'
  let t = ''
  for (let r = 0; r < b; r = r + 1) {
    for (let k = 0; k < 2; k = k + 1) t = t + spaces(s) + ' '
  }
  let total = 0
  for (let q = 0; q < c; q = q + 1) {
    for (let k = 0; k < 3; k = k + 1) {
      for (let z = 0; z < 2; z = z + 1) total = total + spaces(t)
    }
  }
  return s.length + '|' + t + '|' + total
}

// read inside the loop bodies, the loops that are read the larger
function bodyReadsDown(a: number, b: number, c: number): string {
  let s = ''
  for (let i = 0; i < a; i = i + 1) {
    for (let k = 0; k < 2; k = k + 1) {
      if (k === 0) s = s + 'a b'
      else s = s + 'c'
    }
  }
  let t = ''
  for (let r = 0; r < b; r = r + 1) {
    if (r % 2 === 0) t = t + spaces(s) + ' '
    else t = t + '. '
  }
  let total = 0
  for (let q = 0; q < c; q = q + 1) total = total + spaces(t)
  return s.length + '|' + t + '|' + total
}

// carried into the next loop through its entry, in loop order
function entryReads(n: number): number {
  let acc = 1
  for (let i = 0; i < n; i = i + 1) acc = acc + i
  for (let j = 0; j <= n; j = j + 1) acc = acc + j * acc % 7
  for (let k = 0; k < n; k = k + 1) acc = acc * 2 % 1000 + k
  return acc
}

// carried through the entries, the later loops the larger
function entryReadsUp(n: number): number {
  let acc = 1
  for (let i = 0; i < n; i = i + 1) acc = acc + i
  for (let j = 0; j <= n; j = j + 1) {
    if (j % 3 === 0) acc = acc + j * acc % 7
    else acc = acc + 1
  }
  for (let k = 0; k < n; k = k + 1) {
    if (k % 2 === 0) acc = acc * 2 % 1000 + k
    else if (k % 5 === 0) acc = acc - 3
    else acc = acc + k % 4
  }
  return acc
}

const sizes = [['3', '2', '2'], ['3.5', '2', '1'], ['2', '2.5', '2'], ['0', '3', '1'], ['x', '2', '2'], ['4', '0', '3']]
for (let i = 0; i < sizes.length; i = i + 1) {
  const a = Number(sizes[i][0])
  const b = Number(sizes[i][1])
  const c = Number(sizes[i][2])
  console.log(bodyReads(a, b, c), bodyReadsDown(a, b, c))
}

const bounds = ['6', '6.5', '0', '-1', 'x', '11']
for (let i = 0; i < bounds.length; i = i + 1) {
  const n = Number(bounds[i])
  console.log(bounds[i], entryReads(n), entryReadsUp(n))
}
