// exported enums from another module: a merged one, one with computed
// members (their initializers run with the module body), a renamed
// value import, and a local enum of the same name as the imported one
import { Mode, Comp, Color as C, hidden } from "./lib";

enum Color {
  Red = 10,
}

let m: Mode = Mode.Off;
console.log(Mode.On, m, Object.keys(Mode).join(","));
let x: Comp = Comp.Y;
console.log(Comp.X, x, Comp[84], Object.keys(Comp).join(","));
console.log(C.Red, C.Green, C[2], Color.Red, Color[10], hidden());
