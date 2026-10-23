// A combinator's result is resolved through PromiseResolve (§27.2.4.7),
// which adopts any thenable, including an array once Array.prototype
// has a callable then. Each entry that can produce such a result must
// keep that probe: the race result is the element itself, and the
// iterable entries take their input as any.

;(Array.prototype as any).then = function (res: any) {
  res("ADOPTED")
}
Promise.race([Promise.resolve([1])]).then((v: any) => console.log("race", v))
const it: any = new Set([1])
Promise.all(it).then((v: any) => console.log("dyn", v))
const keyed: any = new Map([["k", 1]]).values()
Promise.allSettled(keyed).then((v: any) => console.log("keyed", v))
