// a `var` export requested a second time (after landing unrequested on
// the first walk) binds to the landed declaration; its initializer runs
// once
import { other } from "./lib";
import { rr } from "./mid";
console.log(other, rr);
