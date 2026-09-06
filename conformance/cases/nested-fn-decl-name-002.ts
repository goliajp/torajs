// 604-03 taught the fn-name registry that §B.3.3's hoist renames a
// nested `function f`, so `(Foo as any).name` answers `Foo` and not
// the mint `__nested_outer_Foo_0`. It left the other reader alone:
// when the object slot is statically a function, the member read is
// folded at compile time and never reaches the registry — so a bare
// `Foo.name` still answered the mint. Same table, both readers.

function outer(): void {
  function Foo(): void {}
  console.log(Foo.name);
  console.log((Foo as any).name);
}
outer();

// The IIFE body is itself a lifted closure, so the mint nests one
// level deeper (`__nested___closure_0_Foo_0`).
(function () {
  function Foo(): void {}
  console.log(Foo.name);
})();

// Two deep: the parent half of the inner mint is itself a mint.
function deep(): void {
  function mid(): void {
    function inner(): void {}
    console.log(inner.name);
  }
  mid();
  console.log(mid.name);
}
deep();

// The neighbouring folds on the same gate must not move: `.length`
// counts source params, `String(f)` prints the source spelling, and
// a top-level declaration was never renamed.
function arity(): void {
  function Foo(a: number, b: number): void {}
  console.log(Foo.length, String(Foo));
}
arity();

function plain(): void {}
console.log(plain.name, plain.length);
