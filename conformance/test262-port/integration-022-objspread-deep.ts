// Integration: object spread combined with member access patterns.
// Exercises the static-shape struct merge across all-Copy and
// mixed-Copy struct shapes. Multi-spread-source-in-same-scope
// combinations test the consume-on-spread machinery.
type Cfg = { name: string, retries: number, verbose: boolean };
type CfgX = { name: string, retries: number, verbose: boolean, host: string };

function check(): number {
  // Simple struct extension.
  let base: Cfg = { name: "base", retries: 3, verbose: false };
  let extended: CfgX = { ...base, host: "localhost" };
  if (extended.name !== "base") { throw "#1"; }
  if (extended.retries !== 3) { throw "#2"; }
  if (extended.verbose !== false) { throw "#3"; }
  if (extended.host !== "localhost") { throw "#4"; }

  // Override on string field.
  let renamed: Cfg = { name: "renamed", retries: 0, verbose: true };
  let final_cfg: Cfg = { ...renamed, name: "final" };
  if (final_cfg.name !== "final") { throw "#5"; }
  if (final_cfg.retries !== 0) { throw "#6"; }
  if (final_cfg.verbose !== true) { throw "#7"; }
  return 0;
}
console.log(check());
