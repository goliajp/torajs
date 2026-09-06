// The `const` half of the same rule, and the half that must NOT
// change: an unshadowed native error is still the injected class.

const RangeError = function (): void {};
const made: any = new (RangeError as any)();
console.log(made.constructor === RangeError);

// SyntaxError is not shadowed here, so it is still the real one.
try {
  throw new SyntaxError("boom");
} catch (e) {
  console.log(e instanceof SyntaxError, (e as Error).message, (e as Error).name);
}

// And a shadowed name does not disturb its siblings' hierarchy.
try {
  throw new TypeError("t");
} catch (e) {
  console.log(e instanceof TypeError, e instanceof Error);
}
