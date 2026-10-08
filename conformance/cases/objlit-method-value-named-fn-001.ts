// a method value read off an object literal, then called with a named
// function: the closure parameter receives the function as a cell
const q = { m(cb: (v: number) => void) { cb(40) } }
const m = q.m
function nf(v: number) { console.log(v) }
m(nf)
