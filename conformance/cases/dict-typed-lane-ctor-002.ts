// A run-time key put together from pieces may still be
// `constructor`: the dictionary read then answers the inherited
// `Object` (§20.1.3.1), and the statics read off that answer are the
// functions `Object` owns. No literal in the program spells the key,
// and nothing else names a constructor or a namespace.
const d: Record<string, number> = { a: 1, b: 2 }
const parts: string[] = ["constr", "uctor"]
const k = parts[0] + parts[1]
const O: any = d[k]
console.log(typeof O.keys, O.keys(d).join(","), O.entries(d).length)
const P: any = d[parts[1]]
console.log(P === undefined)
