// A lib whose exports depend on its own top-level statements.
let counter = 10;
console.log("lib init");
counter = counter + 1;
export let n = 0;
n = n + 5;
console.log("lib n " + n);
export function get(): number { return counter; }
export function bump(): void { n = n + 1; }
