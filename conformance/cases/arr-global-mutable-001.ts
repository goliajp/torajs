// K.6 close — mutable (let/var) Arr globals promote to real slots so
// named-fn bodies can read, mutate, and reassign them. B1 fixed the
// arr cell across growth (push reallocs only the spilled data
// buffer), so method mutation needs no slot writeback; whole-binding
// reassignment rides the Assign-Ident global lane's
// drop-old/store-new.
let xs: number[] = [3, 1];
var ys: number[] = [10];
function readFirst(): number { return xs[0] }
function pushAndLen(): number { xs.push(5); return xs.length }
function reassign(): void { xs = [7, 8, 9] }
console.log(readFirst())
console.log(pushAndLen())
console.log(xs[2])
reassign()
console.log(xs[0], xs.length)
function bumpYs(): number { ys.push(20); return ys[1] }
console.log(bumpYs())
console.log(ys.length)

// refcounted-element lane: string[] global — push mints, reassign
// drops the old cell (element walk), for-of reads the new one.
let ss: string[] = ["a", "b"];
function readS(): string { return ss[1] }
function growS(): number { ss.push("c"); return ss.length }
console.log(readS())
console.log(growS())
ss = ["z"]
console.log(ss[0], ss.length)
for (const s of ss) console.log(s)
