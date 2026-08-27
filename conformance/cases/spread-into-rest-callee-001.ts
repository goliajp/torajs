// A spread stands for an unknown number of arguments, so it settles no
// position: `g(...xs)` on `g(x, ...r)` binds `x` to `xs[0]` and the
// rest to the tail, which no static packing can spell. The wrap that
// routes such a site to the runtime spread lane was counting the
// spread itself toward the callee's required prefix, stood aside for a
// static expander that then also declined, and the bare function name
// reached a lane that cannot box it — `unknown ident \`g\`` on ordinary
// JavaScript.
function g(x, ...r) {
  return "g|" + x + "|" + r.join(",")
}
const a: any[] = [1, 2, 3]
console.log(g(...a))

// a spread that arrives AFTER the required prefix is settled is still
// the static expander's, and still right
const tail: any[] = [2, 3]
console.log(g(1, ...tail))

// two required parameters ahead of the tail
function h(x, y, ...r) {
  return "h|" + x + "|" + y + "|" + r.join(",")
}
console.log(h(...[1, 2, 3, 4] as any[]))

// nothing but a tail
function k(...r) {
  return "k|" + r.join(",")
}
console.log(k(...a))

// the spread runs out before the required prefix does
const short: any[] = [1]
console.log(g(...short))
const none: any[] = []
console.log(g(...none))

// a spread followed by more arguments
console.log(g(...tail, 9))

// two spreads
const first: any[] = [1, 2]
const second: any[] = [3, 4]
console.log(g(...first, ...second))

// a typed source, and a callee whose body does arithmetic on the head
function sum(x: number, ...r: number[]) {
  return x + r.length
}
const nums: number[] = [5, 6, 7]
console.log(sum(...nums))
