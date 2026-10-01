// slot holes of an Any cell (new Array(n), an elision) reach the
// map lane's hole arm through the holey bit
const a = new Array(3)
a[1] = 'x'
let calls = 0
const b = a.map((v: any) => { calls = calls + 1; return v + v })
console.log(calls, b.length, 0 in b, 1 in b, 2 in b, JSON.stringify(b))
const e: any[] = [7, , 9]
const f = e.map((v: any) => v - 1)
console.log(f.length, 1 in f, JSON.stringify(f))
