export async function af(): Promise<number> {
  return 42;
}
export function* gf([a, b]: number[] = [1, 2]) {
  yield a + b;
}
export async function* agf() {
  yield 7;
}
