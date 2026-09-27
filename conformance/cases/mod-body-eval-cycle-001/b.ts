import { fa } from "./a.ts";
console.log("b body " + typeof fa);
export function fb(): string { return "fb+" + fa(); }
