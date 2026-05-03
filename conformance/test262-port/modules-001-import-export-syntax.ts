// Phase K.1 — single-file mode: export syntax accepted but has no
// semantic effect (the module-level export marker is stripped at
// desugar time). Cross-file linking lands in K.2-K.4. Imports
// excluded from this test because bun's runtime resolves the module
// path; tr's K.1 just parses-and-ignores. This test verifies the
// export-modifier shapes (function / const / type / class / named
// re-export) parse, type-check, and run identically to the
// unmodified versions.

// `export` modifier on each declaration shape (function / type /
// class / const-with-literal). Top-level `const` with a literal
// initializer is registered as a global so named-fn bodies can
// read it; non-literal initializers stay scoped to the implicit
// main fn.
export function greet(): string {
  return "hi";
}

export const X: number = 42;
export const TAG: string = "marker";
export const ON: boolean = true;

export type Item = { name: string, count: number };

export class Box {
  contents: number;
  constructor(c: number) {
    this.contents = c;
  }
  read(): number {
    return this.contents;
  }
}

function check(): number {
  if (greet() !== "hi") { throw "#1"; }
  if (X !== 42) { throw "#2: X"; }
  if (TAG !== "marker") { throw "#3: TAG"; }
  if (!ON) { throw "#4: ON"; }
  let it: Item = { name: "apple", count: 5 };
  if (it.name !== "apple") { throw "#5"; }
  let b1: Box = new Box(7);
  if (b1.read() !== 7) { throw "#6"; }
  return 0;
}
console.log(check());
