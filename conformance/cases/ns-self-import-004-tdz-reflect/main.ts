// the entry's own namespace read before its exports initialize: every
// face that needs the binding's value (a get, a descriptor, keys,
// hasOwnProperty, propertyIsEnumerable, for-in) is the ReferenceError
// of the dead zone; has / ownKeys answer without reading the value
import * as ns from "./main";
function probe(f: () => any): string { try { f(); return "ok"; } catch (e: any) { return e.constructor.name; } }
console.log(probe(() => ns.local1), probe(() => ns.renamed), probe(() => ns.default), probe(() => ns.indirect));
console.log(probe(() => Object.keys(ns)), probe(() => Object.prototype.hasOwnProperty.call(ns, "local1")), probe(() => Object.prototype.propertyIsEnumerable.call(ns, "local1")), probe(() => Object.getOwnPropertyDescriptor(ns, "local1")));
console.log("local1" in ns, Reflect.has(ns, "local1"), Object.getOwnPropertyNames(ns).join(","), probe(() => { for (const k in ns) { } }), probe(() => Object.entries(ns)));
export let local1 = 23;
let local2 = 45;
export { local2 as renamed };
export { local1 as indirect } from "./main";
export default null;
console.log(ns.local1, ns.renamed, ns.indirect, ns.default, Object.keys(ns).join(","), probe(() => Object.keys(ns)));
