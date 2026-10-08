export function base(): number {
  return 41;
}

console.log("lib loaded");

export enum Mode {
  On = "on",
}

export enum Mode {
  Off = "off",
}

export enum Comp {
  X = base() + 1,
  Y = X * 2,
}

export enum Color {
  Red = 1,
  Green,
}

enum Hidden {
  Seven = 7,
}

export function hidden(): number {
  return Hidden.Seven;
}
