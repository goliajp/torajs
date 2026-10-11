import { value } from "./a.ts";
console.log("b body " + value());
const items: number[] = [];
items.push(value() + 1);
export default items;
