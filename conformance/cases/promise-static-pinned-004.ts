// the constructor reached under another name: an alias, an instance's
// `constructor`, the global object
const P: any = Promise
P.resolve = function (v: any) {
  console.log("alias", v)
  return new Promise((res: any) => res("a:" + v))
}
Promise.resolve(1).then((v: any) => console.log("one", v))

const inst: any = Promise.reject(0)
inst.catch(() => {})
inst.constructor.reject = function (v: any) {
  console.log("ctor", v)
  return new Promise((res: any) => res("c:" + v))
}
Promise.reject(2).then((v: any) => console.log("two", v))

;(globalThis as any).Promise.resolve = function (v: any) {
  console.log("global", v)
  return new Promise((res: any) => res("g:" + v))
}
Promise.resolve(3).then((v: any) => console.log("three", v))
