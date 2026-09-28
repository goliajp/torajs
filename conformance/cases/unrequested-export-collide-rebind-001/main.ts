// unrequested exports whose names the entry also binds, while the lib
// rebinds those names as parameters: the declarations still land (their
// initializers run), top-level reads and self-references follow them,
// and the parameter sites keep their own binding
import { other, shown } from "./lib";
const r = "mine";
const f = "mine-f";
console.log("main", other, r, f, shown());
