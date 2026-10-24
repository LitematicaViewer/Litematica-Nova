import { listenEvent } from "../../src/platform/events";
import { getUserConfig, type MotionPreference } from "../../src/platform/appData";

export type EffectiveMotion = Exclude<MotionPreference, "system">;
export type MotionState = { preference: MotionPreference; effective: EffectiveMotion; systemReduced: boolean };
type MotionConfig = { config: { motion_preference?: string } };
let preference: MotionPreference = "system";
let systemQuery: MediaQueryList | undefined;
let initialized: Promise<void> | undefined;
let revision = 0;
const subscribers = new Set<() => void>();

export const normalizeMotionPreference = (value: unknown): MotionPreference =>
  value === "off" || value === "reduced" || value === "standard" || value === "rich" ? value : "system";
export const resolveMotionPreference = (value: MotionPreference, reduced: boolean): EffectiveMotion =>
  value === "system" ? reduced ? "reduced" : "standard" : value;
export const currentMotionState = (): MotionState => ({
  preference, effective: resolveMotionPreference(preference, systemQuery?.matches ?? false), systemReduced: systemQuery?.matches ?? false,
});
const apply = () => {
  const state = currentMotionState();
  document.documentElement.dataset.motionPreference = state.preference;
  document.documentElement.dataset.motion = state.effective;
  for (const callback of subscribers) callback();
};
/** Only a successful Rust config read/save/broadcast changes the preference. */
export const acceptMotionConfig = (info: MotionConfig) => {
  revision++;
  preference = normalizeMotionPreference(info.config.motion_preference);
  apply();
};
export const subscribeToMotionChanges = (callback: () => void) => {
  subscribers.add(callback);
  return () => { subscribers.delete(callback); };
};

/** Called by the shared window bootstrap, including every independent WebView. */
export const initializeWindowMotion = (readSavedConfig: () => Promise<MotionConfig> = getUserConfig): Promise<void> => {
  if (!initialized) initialized = (async () => {
    systemQuery = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    systemQuery?.addEventListener("change", apply);
    apply();
    const beforeRead = revision;
    await listenEvent<MotionConfig>("user-config-changed", event => acceptMotionConfig(event.payload)).catch(console.warn);
    try {
      const info = await readSavedConfig();
      if (revision === beforeRead) acceptMotionConfig(info);
    } catch (error) { console.warn(error); }
  })();
  return initialized;
};

/** Presentation only: no route timer, cloned Viewer, delayed command or unmount. */
export const animatePageChange = (element: HTMLElement | null): (() => void) => {
  const state = currentMotionState();
  if (!element?.animate || state.effective === "off") return () => {};
  const styles = getComputedStyle(element);
  // Child HWNDs do not inherit a WebView transform. Never move the DOM anchor
  // underneath a visible native surface or make geometry follow animation frames.
  const nativeSurface = Array.from(element.querySelectorAll?.(".render-preview-host, .properties-preview-host, .layer3d-viewport, .composition-native-viewport") ?? [])
    .some(host => !host.closest("[hidden]"));
  const distance = state.effective === "reduced" || nativeSurface ? "0px" : styles.getPropertyValue("--nova-motion-page-distance").trim() || "8px";
  const duration = parseFloat(styles.getPropertyValue("--nova-motion-page-duration")) || (state.effective === "reduced" ? 120 : 180);
  const keyframes = distance === "0px" ? [{ opacity: .72 }, { opacity: 1 }] : [
    { opacity: .72, transform: `translateY(${distance})` }, { opacity: 1, transform: "translateY(0)" },
  ];
  const animation = element.animate(keyframes, { duration, easing: styles.getPropertyValue("--nova-motion-ease").trim() || "cubic-bezier(0.16, 1, 0.3, 1)" });
  const off = subscribeToMotionChanges(() => { animation.cancel(); });
  return () => { animation.cancel(); off(); };
};
