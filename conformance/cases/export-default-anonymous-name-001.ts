// §16.2.3.7: `export default` of an anonymous class or function definition is a
// NamedEvaluation with "default"; a definition with its own name keeps it.
export default class { v() { return 1; } }
export const x = 0;
import("./export-default-anonymous-name-001.ts").then((m: any) => {
  console.log(m.default.name, new m.default().v(), m.x);
});
