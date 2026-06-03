// Side-effect lib for mod-side-effect-import-001. Emits in source order
// when imported as `import "./lib"`; pre-fix tora rejected this form.
console.log("lib-side-1");
console.log("lib-side-2");
const computed: number = 10 * 4 + 2;
console.log("lib-computed", computed);
