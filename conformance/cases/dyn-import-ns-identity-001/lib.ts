console.log("lib body");
export let count = 0;
export function bump() { count++; }
export * as inner from "./inner";
