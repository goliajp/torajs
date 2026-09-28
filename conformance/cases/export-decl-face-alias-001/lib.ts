export let count = 0;
export function bump() { count++; }
export function peek() { return count; }
export { count as total, bump as inc };
export class K { static s = 1; }
export { K as Klass };
