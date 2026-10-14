// §10.1.8.1 OrdinaryGet / §10.1.9.2 OrdinarySet on a dynamic string
// key: an own entry decides both the read and the write alone, an
// own entry that STORES undefined is still own (it shadows what the
// prototype would answer), and an absent key reads undefined.
const keys: string[] = ["a", "b", "toString", "1", "01", "x"]
const o: any = {}
o[keys[0]] = undefined
o[keys[3]] = 5
console.log(o[keys[0]], keys[0] in o, Object.keys(o).length)
console.log(typeof o[keys[2]])
o[keys[2]] = undefined
console.log(o[keys[2]], Object.keys(o).join(","))
console.log(o[keys[3]], o[1], o[keys[4]])
o[keys[4]] = "z"
console.log(o[keys[4]], o["01"], Object.keys(o).join(","))
console.log(o[keys[5]], keys[5] in o)
// counting loop: read-modify-write of the same key
const counts: Record<string, number> = {}
for (let i = 0; i < 20; i = i + 1) {
  const k = keys[i % 3]
  counts[k] = (counts[k] ?? 0) + 1
}
console.log(JSON.stringify(counts))
// delete, then write again: the key comes back at the end
const d: any = { p: 1, q: 2, r: 3 }
const kq: string = keys[1] === "b" ? "q" : "p"
delete d[kq]
console.log(d[kq], JSON.stringify(d))
d[kq] = 9
console.log(d[kq], JSON.stringify(d), Object.keys(d).join(","))
d[kq] = d[kq] + 1
console.log(d[kq])
