// Every way a string's length is read must see the same code-unit
// count whatever the allocator keeps in the header word beside it:
// a fresh short concat, a cell grown by appends, a long one, a
// UTF-16 one. The slice / substring / substr arms that default their
// end to the length, and an end that is only undefined at run time,
// read it straight off the cell.
const tail = process.argv.length > 99 ? 'never' : 'x'
const pieces: string[] = []
const short = 'ab' + tail
pieces.push(short)
let grown = ''
for (let i = 0; i < 40; i = i + 1) {
  grown = grown + 'q'
}
pieces.push(grown)
const long = 'the quick brown fox jumps over the lazy dog, twice: ' + tail + tail
pieces.push(long)
const wide = '中文' + tail
pieces.push(wide)
const mid = 'abcdefghijklmnopqrstuvwxyz' + tail
pieces.push(mid)

const ends: any[] = [undefined, 3]
for (let k = 0; k < pieces.length; k = k + 1) {
  const s = pieces[k]
  const row: string[] = []
  row.push('' + s.length)
  row.push(s.slice(1))
  row.push(s.slice())
  row.push(s.substring(2))
  row.push(s.substring())
  row.push(s.substr(1))
  row.push(s.slice(1, undefined))
  row.push(s.substring(1, undefined))
  for (let e = 0; e < ends.length; e = e + 1) {
    row.push(s.slice(1, ends[e]))
    row.push(s.substring(1, ends[e]))
    row.push(s.substr(1, ends[e]))
  }
  console.log(k, row.join('|'))
}

// cells released and taken again across the pool's classes
let churn = 0
for (let i = 0; i < 300; i = i + 1) {
  const a = 'k' + i
  const b = a + a + a + a + a + a + a + a
  const c = b + b + b
  churn = churn + a.length + b.length + c.length + c.charCodeAt(c.length - 1)
}
console.log('churn', churn)
