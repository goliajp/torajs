// symbol-keyed reads on an ordinary object: own entries answer from
// one probe, absent keys keep walking the prototype chain
const n = parseInt(process.argv[2] ?? "3", 10)
const s1 = Symbol("one")
const s2 = Symbol("two")
const s3 = Symbol("three")
const o: any = {}
o[s1] = 10
o[s2] = undefined
console.log(o[s1], o[s2], s2 in o, o[s3], s3 in o)
// own data entry, read in a loop and overwritten
let sum = 0
for (let i = 0; i < n; i = i + 1) {
  o[s1] = o[s1] + i
  sum = sum + o[s1]
}
console.log(sum, o[s1])
// own accessor entry: the getter runs once per read, with the object as this
let calls = 0
const acc: any = { base: 7 }
Object.defineProperty(acc, s3, {
  get(this: any) {
    calls = calls + 1
    return this.base * 2
  },
  configurable: true,
})
console.log(acc[s3], acc[s3], calls)
// inherited through a user prototype, then shadowed by an own entry
const parent: any = {}
parent[s1] = "from parent"
const child: any = Object.create(parent)
console.log(child[s1])
child[s1] = "own"
console.log(child[s1], parent[s1])
delete child[s1]
console.log(child[s1])
// a null prototype ends the walk
const bare: any = Object.create(null)
console.log(bare[s1])
bare[s1] = "bare"
console.log(bare[s1])
// inherited from Object.prototype
Object.defineProperty(Object.prototype, s2, { value: "root", configurable: true, writable: true })
const plain: any = {}
console.log(plain[s2], o[s2])
delete (Object.prototype as any)[s2]
console.log(plain[s2])
// well-known symbol on an object that owns none
const arrLike: any = { length: 0 }
console.log(typeof arrLike[Symbol.iterator], typeof [][Symbol.iterator])
// frozen object still reads its own symbol entries
const frozen: any = Object.freeze({ [s1]: "ice" })
console.log(frozen[s1], frozen[s2])
