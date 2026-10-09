// Lib for binding-shadows-global-fn-003: exports named like global
// function properties.
export function parseInt(s: string): number {
  return s.length * 1000;
}
export function escape(n: number): number {
  return n + 0.5;
}
