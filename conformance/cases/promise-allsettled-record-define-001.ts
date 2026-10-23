// allSettled builds each record with CreateDataPropertyOrThrow
// (§27.2.4.2.2 step 10, §27.2.4.2.3 step 11), a [[DefineOwnProperty]]:
// a setter for the same key on Object.prototype is never called.

Object.defineProperty(Object.prototype, "status", {
  set(_v: any) {
    console.log("WRONG setter")
  },
  configurable: true,
})
const input: any = [Promise.resolve(1), Promise.reject(new Error("e")), 3]
Promise.allSettled(input).then((rs: any) => {
  for (const r of rs) console.log(r.status, r.value === undefined ? r.reason.message : r.value)
})
