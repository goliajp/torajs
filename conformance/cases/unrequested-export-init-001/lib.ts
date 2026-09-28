export const other = 1;
const helper = () => (console.log("helper ran"), 5);
export const r = helper();
export let counter = (console.log("init counter"), 10);
export function unused() { return counter; }
console.log("lib body", r);
