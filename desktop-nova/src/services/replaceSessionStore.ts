/**
 * Session-level storage for Replace page form data.
 * Data persists across page navigation but is cleared on app restart.
 */

import type { ReplaceItem } from '../business/replace/types';

interface ReplaceSessionData {
  items: ReplaceItem[];
  selectedPreset: string;
  outputPath: string;
}

// In-memory store that lives for the application lifetime
let sessionStore: ReplaceSessionData | null = null;

/**
 * Save current replace form state to session store.
 */
export function saveReplaceSession(data: ReplaceSessionData): void {
  sessionStore = data;
}

/**
 * Load replace form state from session store.
 * Returns null if no data was previously saved.
 */
export function loadReplaceSession(): ReplaceSessionData | null {
  return sessionStore;
}

/**
 * Clear the session store (typically called on explicit user action, not on navigation).
 */
export function clearReplaceSession(): void {
  sessionStore = null;
}
