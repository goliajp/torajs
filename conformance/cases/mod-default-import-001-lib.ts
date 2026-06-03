// Lib for mod-default-import-001 fixture. Tests `export default` + the
// importer's `import x from "./lib"` (P13-S1).
export default function(): string {
  return "default-result";
}
export const NAMED_X = 42;
