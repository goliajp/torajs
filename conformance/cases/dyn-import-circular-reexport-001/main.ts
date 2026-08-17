// 426-01 — §16.2.1.5: a dyn-import candidate whose indirect exports
// never resolve (circular `export { x } from` chain, or a missing
// source binding) REJECTS its promise with a SyntaxError instead of
// failing the build. A healthy candidate in the same program still
// resolves its namespace.
import('./circ-a.ts')
  .then(() => console.log("circ resolved (wrong)"))
  .catch((e: any) => console.log("circ caught:", e.name))
  .then(() => import('./ok.ts'))
  .then((ns: any) => console.log("ok:", ns.x, ns.y))
  .then(() => import('./miss-a.ts'))
  .then(() => console.log("miss resolved (wrong)"))
  .catch((e: any) => console.log("miss caught:", e.name));
