import type { ApplicationRenderSnapshot, RenderOperationView } from "../platform/applicationRender";

// Compatibility for analysis consumers: no Full service is started or queried.
// Consumers receive no render-derived analysis and use the real project-statistics API.
const snapshot: ApplicationRenderSnapshot = { operations: [] };
export function ensureApplicationRenderSubscription(): Promise<void> { return Promise.resolve(); }
export function subscribeApplicationRender(_listener: () => void): () => void { return () => {}; }
export function getApplicationRenderState(): ApplicationRenderSnapshot { return snapshot; }
export function isApplicationRenderStateReady(): boolean { return true; }
export function findRenderOperationForSource(_path: string, _source = snapshot): RenderOperationView | null { return null; }
export type { ApplicationRenderSnapshot, RenderOperationView } from "../platform/applicationRender";
