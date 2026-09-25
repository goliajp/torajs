// toLocaleString on a holey typed array walks the exotic join, which
// re-dispatches toLocaleString per element; the families it reaches
// must survive the link judgment

const b: number[] = [0.5, 2]
delete b[0]
console.log(b.toLocaleString())

const g: number[] = [0.5, 1]
g.length = 4
console.log(g.toLocaleString())

const s: string[] = ["x", "y"]
delete s[1]
console.log(s.toLocaleString())
