// Hub: everything `a` exports joins THIS module's export face, so a
// namespace object built for the hub has to carry a's names too.
export * from "./a.ts";
export const NB = "nb";
