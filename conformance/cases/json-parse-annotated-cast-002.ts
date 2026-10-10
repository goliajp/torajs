// `await Bun.file(p).json()` into an annotated binding answers the way
// `JSON.parse` of the same text does: an object type whose fields come
// in another order still fits, and a class annotation is a cast of the
// program's own type, judged by name, which a parsed plain object never
// passes (bun keeps the plain object here; this line is the project's
// decision for written class annotations)
import { writeFile, unlink } from "fs/promises";

const path = "/tmp/torajs-json-annotated-cast-002.json";
await writeFile(path, '{"version":42,"name":"t"}');
type Pkg = { name: string; version: number };
const pkg: Pkg = await Bun.file(path).json();
console.log(pkg.name, pkg.version);
class P {
  name = "";
  version = 0;
}
let r = "";
try {
  const p: P = await Bun.file(path).json();
  r = "kept " + p.name;
} catch (e) {
  r = "threw " + (e instanceof TypeError);
}
console.log(r === "kept t" || r === "threw true");
await unlink(path);
