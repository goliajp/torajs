// a direct write of the static slot, in a program with no other road
// to the constructor: the call sites before it keep the builtin, the
// ones after it run the patch
let calls = 0
function settle(v: number): void {
  console.log("settled", v, calls)
}
Promise.reject(1).catch(settle)
function patch(): void {
  Promise.reject = function (x: any): any {
    calls++
    return new Promise((res) => res(x))
  } as any
}
patch()
Promise.reject(2).then(settle)
