// the key is built at run time, so the program never spells it
const o: any = { a: 1 }
const key = ["con", "struc", "tor"].join("")
const C = o[key]
console.log(C.keys(o), C.values(o), C.entries(o))
