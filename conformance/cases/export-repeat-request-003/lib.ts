let count = 0;
function bump() { count++; }
export function f() { return 7; }
export class K { static s = 1; }
export { count, count as total, bump };
