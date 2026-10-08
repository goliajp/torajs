import { Color, Weight, Label, describe, firstColor } from "./colors";

let c: Color = Color.Blue;
console.log(Color.Red, Color.Green, c, firstColor);
console.log(Color[0], Color[c], Object.keys(Color));
console.log(describe(Color.Green), describe(c));
console.log(Weight.Light, Weight.Heavy, Weight[900]);
console.log(Label.On, Label.Off, Object.keys(Label));

function weigh(w: Weight): string {
  return w === Weight.Heavy ? "heavy" : "light";
}
console.log(weigh(Weight.Heavy), weigh(Weight.Light));

let lbl: Label = Label.Off;
switch (lbl) {
  case Label.On:
    console.log("on!");
    break;
  case Label.Off:
    console.log("off!");
    break;
}
console.log(typeof Color, typeof Weight, typeof Label);
