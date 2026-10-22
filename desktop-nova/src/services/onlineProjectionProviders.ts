import { t } from "./i18n";
/** Adapters come from observed frontend requests/HTML, not a public-documentation prerequisite. */
export interface OnlineProjectionProvider {
  id: string;
  name: string;
  url: string;
  mode: "native" | "website";
  note: string;
}

export const ONLINE_PROJECTION_PROVIDERS: readonly OnlineProjectionProvider[] = [
  { id: "reden", name: "RedenMC", url: "https://redenmc.com/", mode: "native", get note() { return t("native.provider.reden"); } },
  { id: "minemev", name: "Minemev", url: "https://www.minemev.com/", mode: "native", get note() { return t("native.provider.minemev"); } },
  { id: "mcodex", name: "MCodex", url: "https://mcodex.org/", mode: "native", get note() { return t("native.provider.mcodex"); } },
  { id: "choculaterie", name: "Choculaterie", url: "https://choculaterie.com/", mode: "website", get note() { return t("native.provider.choculaterie"); } },
  { id: "projection-workshop", get name() { return t("native.provider.workshopName"); }, url: "https://www.mcimg.com/", mode: "native", get note() { return t("native.provider.workshop"); } },
  { id: "abfielder", name: "Abfielder", url: "https://abfielder.com/", mode: "native", get note() { return t("native.provider.abfielder"); } },
  { id: "mineschematic", name: "MineSchematic", url: "https://mineschematic.com/", mode: "website", get note() { return t("native.provider.mineschematic"); } },
  { id: "shulkr", name: "Shulkr", url: "https://www.shulkr.com/en", mode: "website", get note() { return t("native.provider.shulkr"); } },
];
