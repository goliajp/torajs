import { leaf } from "./leaf.ts";
import * as ns from "./leaf.ts";
export let counter = 0;
export function bump() {
  counter = counter + 1;
}
export function tryWrite(): string {
  try {
    leaf = 5;
    return "no throw";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}
export function tryIncr(): string {
  try {
    ns = null;
    return "no throw";
  } catch (e) {
    return e instanceof TypeError ? "TypeError" : "other";
  }
}
