export let n = 0;
export function bump() { n++; }
export { n as m, bump as inc };
