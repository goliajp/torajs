// §16.2.3 — `export * from` never forwards `default`, in either
// spelling of the source module (`export { x as default }` or
// `export default`), even though the source module evaluates its default
// export at its position
import * as named from "./sn";
import * as production from "./sp";
console.log("namedOther" in named, "default" in named, "productionOther" in production, "default" in production, Object.keys(named).join(","), Object.keys(production).join(","));
