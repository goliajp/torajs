export enum Color {
  Red,
  Green,
  Blue,
}

export const enum Weight {
  Light = 100,
  Heavy = 900,
}

export enum Label {
  On = "on",
  Off = "off",
}

export function describe(c: Color): string {
  return Color[c] + "/" + c;
}

export const firstColor: Color = Color.Red;
