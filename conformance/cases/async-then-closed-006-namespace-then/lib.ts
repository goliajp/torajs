function f(res: (v: any) => void) {
  res(9);
}

export const a = 5;
export { f as then };
