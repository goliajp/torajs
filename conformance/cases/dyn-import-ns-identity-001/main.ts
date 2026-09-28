// one namespace object per module (§16.2.1.5.3): a dynamic import of a
// module already imported as a namespace answers that same object, and
// the module body still runs once
import * as ns from "./lib";
const dyn = await import("./lib");
const dyn2 = await import("./lib");
const di = await import("./inner");
console.log(dyn === ns, dyn === dyn2, di === ns.inner);
ns.bump();
di.up();
console.log(dyn.count, dyn2.count, ns.inner.n, dyn.inner.n);
