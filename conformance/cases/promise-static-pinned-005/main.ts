// a patch installed by another module reaches this module's typed
// static call sites
import { install } from "./lib"

Promise.resolve(1).then((v: number) => console.log("before", v))
install()
Promise.resolve(2).then((v: any) => console.log("after", v))
