// a patch installed from inside a closure still reaches the typed
// static call sites, before and after the closure runs
function install() {
  (Promise as any).resolve = function (v: any) {
    console.log("patched", v)
    return new Promise((res: any) => res("from-patch:" + v))
  }
}

Promise.resolve(1).then((v: number) => console.log("before", v))
install()
Promise.resolve(2).then((v: any) => console.log("after", v))
