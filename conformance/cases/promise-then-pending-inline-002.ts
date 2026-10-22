// `then` on a pending source when the program replaces `then`: once on
// one promise (an own property, §27.2.5.4 is not reached through it)
// and once on Promise.prototype — every call goes to the program's
// function, never to the built-in reaction.

const log: string[] = []

function add1(v: number): number {
  return v + 1
}

let resolveP: (v: number) => void = (v: number) => {}
const p = new Promise<number>((res) => {
  resolveP = res
})
Object.defineProperty(p, "then", {
  value: function (f: any) {
    log.push("own then")
    return Promise.resolve(f(41))
  },
})
const p1 = p.then(add1)

const origThen = Promise.prototype.then
let calls = 0
;(Promise.prototype as any).then = function (this: any, f: any, r: any) {
  calls = calls + 1
  return origThen.call(this, f, r)
}
let resolveQ: (v: number) => void = (v: number) => {}
const q = new Promise<number>((res) => {
  resolveQ = res
})
const q1 = q.then(add1).then(add1)
resolveQ(1)
resolveP(0)

console.log(await p1)
console.log(await q1)
console.log(log.join(","), calls)
