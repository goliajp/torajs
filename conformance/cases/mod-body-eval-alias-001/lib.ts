export function f(): number { return 1; }
export const k = 10;
function Mercury(): string { return "m"; }
export { Mercury as "☿" };
console.log("lib " + (f() + k) + " " + Mercury());
