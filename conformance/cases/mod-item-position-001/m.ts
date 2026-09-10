// Import target for mod-item-position-001. The default export is
// what `import d, { W }` in main.ts resolves — a module without one
// makes that clause a link-phase SyntaxError (§16.2.1.6.4 step 7),
// which is not the shape this fixture is about.
export default 0;
export const V = 1;
export const W = 2;
