// A program that calls return() by name keeps its unannotated
// generators untyped: return() takes any value or none.
function* g() { yield 1; yield 2 }
const r = g()
console.log(r.next().value)
console.log(JSON.stringify(r.return()))
console.log(JSON.stringify(r.next()))
console.log(JSON.stringify(g().return(7)))
console.log(JSON.stringify(g().return("x")))
const viaAny: any = g()
console.log(JSON.stringify(viaAny.return("y")), JSON.stringify(viaAny.next()))
function* fin() { try { yield 1; yield 2 } finally { console.log("fin") } }
const f = fin()
f.next()
console.log(JSON.stringify(f.return("z")))
