// an out-of-range store far past the end, then reads below and above
// the old length inside the same loop
function run(): number {
  const st: number[] = [0, 0, 0, 0, 0, 0, 0, 0]
  let acc = 0
  for (let i = 0; i < 4; i = i + 1) {
    st[i * 9] = i + 1
    acc = acc + st.length
    acc = acc + st[i * 9]
    const below = st[i * 9 - 1]
    acc = acc + (below === undefined ? 100 : below)
  }
  console.log(st.length, st[27], st[26], st[8])
  return acc
}
console.log(run())
