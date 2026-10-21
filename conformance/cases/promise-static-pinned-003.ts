// Reflect.set and Object.defineProperty write the static slot too
Reflect.set(Promise, "resolve", function (v: any) {
  console.log("set", v)
  return new Promise((res: any) => res("via-set:" + v))
})
Promise.resolve(1).then((v: any) => console.log("resolve", v))

Object.defineProperty(Promise, "reject", {
  value: function (v: any) {
    console.log("defined", v)
    return new Promise((res: any) => res("via-define:" + v))
  },
  writable: true,
  configurable: true,
})
Promise.reject(2).then((v: any) => console.log("reject", v))
