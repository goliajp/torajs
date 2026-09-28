const a = (console.log("init a"), 1);
export { a as b };
export const other = 2;
console.log("lib body", a);
