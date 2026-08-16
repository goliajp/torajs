// `default` as the EXPOSED name: this module's default export IS a's
// `V`. The importer's default binding selects it — `want` never
// contains "default".
export { V as default } from "./a.ts";
export const H = "h";
