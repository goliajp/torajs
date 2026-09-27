// A module that is evaluated can itself import types: the erased
// clause still carries b's type declarations to the checker, and b is
// never requested.
import { f } from "./a.ts";
console.log(f().x);
