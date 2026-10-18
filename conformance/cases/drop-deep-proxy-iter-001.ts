// A Proxy whose target is the previous Proxy, and an iterator helper
// over the previous helper, 300k levels each. Releasing the head
// releases each level in turn; the native depth must stay bounded.
function proxies(n: number): any {
  let p: any = {}
  for (let i = 0; i < n; i++) p = new Proxy(p, {})
  return p
}
function helpers(n: number): any {
  let it: any = [1, 2, 3].values()
  for (let i = 0; i < n; i++) it = it.map((x: number) => x + 1)
  return it
}

let p: any = proxies(300000)
console.log("proxy", typeof p)
p = null
let h: any = helpers(300000)
console.log("helper", typeof h.next)
h = null
console.log("done")
