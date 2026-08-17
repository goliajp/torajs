// §16.2.1.5 — a named `export { x } from` in the ENTRY binds nothing
// anyone can see (nothing imports the entry), but the indirect
// export must still RESOLVE: before the static-resolution knife the
// clause was ignored entirely, so a missing/circular chain sailed
// through silently. Module side-effect statements of a named-request
// walk are an existing subset boundary (dropped for plain named
// imports too), so this fixture pins resolution only.
export { x } from "./base.ts";
console.log("entry-after");
