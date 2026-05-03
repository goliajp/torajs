// Phase K.1 — single-file mode: export syntax accepted but has no
// semantic effect (the module-level export marker is stripped at
// desugar time). Cross-file linking lands in K.2-K.4. Imports
// excluded from this test because bun's runtime resolves the module
// path; tr's K.1 just parses-and-ignores. This test verifies the
// export-modifier shapes (function / const / type / class / named
// re-export) parse, type-check, and run identically to the
// unmodified versions.

// `export` modifier on each declaration shape (function / type /
// class). Top-level `let` / `const` not exercised here — tr's
// pre-K.1 lowering already treats them as locals of the implicit
// main fn so they aren't visible from named fns. K.2 will revisit
// when the cross-file symbol table lands.
export function greet(): string {
  return "hi";
}

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
  let it: Item = { name: "apple", count: 5 };
  if (it.name !== "apple") { throw "#2"; }
  let b1: Box = new Box(7);
  if (b1.read() !== 7) { throw "#3"; }
  return 0;
}
console.log(check());
