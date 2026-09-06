// An alias of a variadic closure binding is itself one: it holds the
// same env cell, so a named-fn body calling it must take the same
// boxed-dual-entry route. Before this, `const gb = ga` was a hard
// reject (K.4 saw two Closure sigs — the slot carries only the fixed
// prefix, the source binding the lifted fn's array-tail sig), and
// when both names were read the alias's call took a static dispatch
// on the arity-0 sig and segfaulted.
const ga = (...xs: number[]): number => xs.length;
const gb = ga;

function viaAlias(): number {
  return gb(1, 2, 3);
}
function viaBoth(): number {
  return ga(1) + gb(1, 2, 3);
}
console.log(viaAlias(), viaBoth(), gb(), gb(9));

// refcounted rest elements through the alias
const js = (...xs: string[]): string => xs.join("-");
const jt = js;
function joined(): string {
  return jt("a", "b", "c");
}
console.log(joined(), jt(), js("z"));

// the source stays callable on the main path and from a named fn
console.log(ga(1, 2), gb(1, 2), viaAlias() === 3);

