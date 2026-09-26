// split results share a small block pool; more results alive at once
// than the pool holds, and results that grow past their split length,
// must release cleanly and leave later splits reading their own parts
const keep: string[][] = []
for (let i = 0; i < 40; i = i + 1) keep.push(("a b c " + i).split(" "))
console.log(keep.length, keep[0].join("|"), keep[39].join("|"))
keep.length = 0
const grown: string[][] = []
for (let i = 0; i < 20; i = i + 1) {
  const p = ("x y " + i).split(" ")
  for (let k = 0; k < 6; k = k + 1) p.push("g" + k)
  grown.push(p)
}
console.log(grown[3].length, grown[3].join(","), grown[19][0], grown[19][8])
grown.length = 0
let acc = ""
for (let r = 0; r < 50; r = r + 1) {
  const parts = ("q" + r + " w e").split(" ")
  acc = acc + parts[0] + parts.length
}
console.log(acc.length, acc.slice(0, 20))
