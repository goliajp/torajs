function next(s: number): number {
  const t = (s * 48271) % 2147483647
  return t === 0 ? 1 : t
}

const n = parseInt(process.argv[2], 10)
const fields = parseInt(process.argv[3], 10)
let seed = parseInt(process.argv[4], 10)

// one record in eight has no = and one in eight has an empty key
const lines: string[] = []
for (let i = 0; i < 1024; i = i + 1) {
  let line = ''
  for (let f = 0; f < fields; f = f + 1) {
    seed = next(seed)
    const shape = seed % 8
    seed = next(seed)
    const klen = 1 + (seed % 8)
    let key = ''
    for (let k = 0; k < klen; k = k + 1) {
      seed = next(seed)
      key = key + String.fromCharCode(97 + (seed % 26))
    }
    seed = next(seed)
    const vlen = 1 + (seed % 6)
    let value = ''
    for (let k = 0; k < vlen; k = k + 1) {
      seed = next(seed)
      value = value + String.fromCharCode(48 + (seed % 10))
    }
    if (shape === 0) line = line + key + value + ';'
    else if (shape === 1) line = line + '=' + value + ';'
    else line = line + key + '=' + value + ';'
  }
  lines.push(line)
}

const re = /([a-z]+)=(\d+)/g
let total = 0
for (let i = 0; i < n; i = i + 1) {
  const line = lines[i & 1023]
  re.lastIndex = 0
  let m = re.exec(line)
  while (m !== null) {
    total = total + m.index + m[1].length + parseInt(m[2], 10)
    m = re.exec(line)
  }
}
console.log(total)
