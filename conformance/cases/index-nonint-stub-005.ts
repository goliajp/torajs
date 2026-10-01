// a match array carries its own props bag (index / input / groups);
// a non-integral index on it still names no property
const hit = /b(c)/.exec('abcd')
if (hit !== null) {
  const key = parseFloat('0.5')
  console.log(hit[key], hit[key + 0.5], hit[key * 0], hit.index)
}
