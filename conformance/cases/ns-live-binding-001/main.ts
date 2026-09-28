// a namespace object's member read is the export's live binding
// (§16.2.1.10), not the value it held when the object was built
import * as ns from "./lib";
import { bump } from "./lib";
console.log(ns.count, ns.total, ns.label);
bump();
ns.bump();
console.log(ns.count, ns.total, ns.default(), typeof ns.K, new ns.K().v, ns.K.s);
const k = Object.keys(ns).sort().join(",");
console.log(k, "count" in ns);
function rd() { return ns.count + ns.total; }
bump();
console.log(rd());
