// `import type` is erased whole under verbatimModuleSyntax: the module
// it names is not requested, so its body never runs, while the type
// declarations it exports still type the importer's code.
import type { Point, Sized, Local } from "./shapes.ts";
const p: Point = { x: 1, y: 2 };
const s: Sized = { size: 3 };
const l: Local = { tag: "t" };
console.log(p.x + p.y, s.size, l.tag);
