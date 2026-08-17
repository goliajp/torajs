export var local1 = "Test262";
var local2 = "TC39";
export { local2 as renamed };
export { local1 as indirect } from "./lib_one.ts";
export default 42;
