// The skins by name (ADR 0041), for the selectors every skin draws. The
// loader is in ./index.ts so a skin can import these without importing itself.

import type { Skin, Snapshot } from '../bindings';

export const SKINS: readonly Skin[] = ['standard', 'panel', 'console'];

export function isSkin(value: string | null | undefined): value is Skin {
  return SKINS.includes(value as Skin);
}

/** The chosen skin; a snapshot from before skins (`#[serde(default)]`) means standard. */
export function skinOf(snapshot: Snapshot): Skin {
  return snapshot.skin ?? 'standard';
}
