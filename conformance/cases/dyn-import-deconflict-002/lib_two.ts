export var local1 = "one six one two";
var local2 = "star";
export { local2 as renamed };
export { local1 as indirect } from "./lib_two.ts";
export default 1612;
