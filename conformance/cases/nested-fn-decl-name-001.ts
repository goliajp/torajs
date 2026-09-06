// §B.3.3's hoist renames a nested `function` declaration so that two
// blocks in one body can each declare the same name. The mint is not
// a name any source can spell, and `.name` used to answer it verbatim
// — `__nested_outer_Foo_0` where the ES answer is `Foo` (§10.2.9
// SetFunctionName runs on the declaration as written).

function outer(): void {
  function Foo(): void {}
  console.log((Foo as any).name);
}
outer();

// The lifted name is also what `toString` and an inspect print show.
function shows(): void {
  function bar(n: number): number {
    return n;
  }
  console.log((bar as any).name, (bar as any).length);
}
shows();

// Two blocks in one body declaring the same name: each keeps the
// source spelling, not its own mint.
function twice(flag: boolean): void {
  if (flag) {
    function pick(): number {
      return 1;
    }
    console.log((pick as any).name, pick());
  } else {
    function pick(): number {
      return 2;
    }
    console.log((pick as any).name, pick());
  }
}
twice(true);
twice(false);

// A declaration nested two deep keeps its own spelling too.
function deep(): void {
  function mid(): void {
    function inner(): void {}
    console.log((inner as any).name);
  }
  mid();
  console.log((mid as any).name);
}
deep();

// A top-level declaration was never renamed and must not change.
function plain(): void {}
console.log((plain as any).name);
