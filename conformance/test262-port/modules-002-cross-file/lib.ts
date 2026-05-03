// Phase K.2 — sibling module imported by main.ts. Exercises every
// declaration shape K.2 supports: function, const-with-literal, type
// alias, and class. Imported decls are injected at the top of main's
// AST before the desugar pipeline runs, so they go through identical
// type-check + lowering as if declared in main.ts directly.

export function add(a: number, b: number): number {
  return a + b;
}

export function mul(a: number, b: number): number {
  return a * b;
}

export const ZERO: number = 0;
export const ONE: number = 1;
export const TAG: string = "lib";

export type Pair = { fst: number, snd: number };

export function makePair(a: number, b: number): Pair {
  return { fst: a, snd: b };
}

export class Counter {
  value: number;
  constructor(start: number) {
    this.value = start;
  }
  inc(): number {
    this.value = this.value + 1;
    return this.value;
  }
}
