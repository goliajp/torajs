// Re-export hub: forwards `fa` plain + `KA as RENAMED_K` aliased.
export { fa } from "./mod-re-export-001-a.ts";
export { KA as RENAMED_K } from "./mod-re-export-001-a.ts";
export function fb(): string { return "fb-result"; }
