export let count = 0;
export const label = "L";
export function bump() { count++; }
export class K { static s = 1; v = 2; }
export { count as total };
export default function tag() { return "t" + count; }
