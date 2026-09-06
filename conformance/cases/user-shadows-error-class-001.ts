// The Error hierarchy is synthesized into the program, and the pass
// steps aside when the program declares the name itself. It asked
// only about `class` declarations, so a `function` / `const` / `let`
// binding of the same name got the injected class spliced in AHEAD of
// it, and every `new <name>()` built the intrinsic instead. Silently:
// the two answer to the same name, so only an identity comparison
// tells them apart.

function TypeError(): void {}
const madeFn: any = new (TypeError as any)();
console.log(madeFn.constructor === TypeError);
console.log(madeFn instanceof TypeError);
console.log((TypeError as any).name);

// The declaration is a plain function, so its instances are not
// errors at all.
console.log(madeFn instanceof Object);
console.log(typeof madeFn.message);
