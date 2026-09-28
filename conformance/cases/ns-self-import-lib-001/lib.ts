import * as ns from "./lib";
export var a_local1;
var local2;
export { local2 as b_renamed };
export { a_local1 as e_indirect } from "./lib";
export * from "./fx";
export function keys(): string { return Object.getOwnPropertyNames(ns).join(","); }
export let c_localUninit1 = 1;
let localUninit2 = 2;
export { localUninit2 as d_renamedUninit };
export { c_localUninit1 as f_indirectUninit } from "./lib";
export default null;
console.log("lib body", ns.c_localUninit1, ns.default);
