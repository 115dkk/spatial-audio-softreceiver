// The skins and how one is loaded (ADR 0041). Each skin is its own chunk with
// its own stylesheet, so a page holds exactly one skin's CSS; changing the
// skin reloads the page (src/main.ts).

import type { Skin } from '../bindings';
import type { SkinModule } from './types';

export type { App, AppOptions, SkinModule } from './types';
export { SKINS, isSkin, skinOf } from './names';

export function loadSkin(skin: Skin): Promise<SkinModule> {
  switch (skin) {
    case 'panel':
      return import('./panel');
    case 'console':
      return import('./console');
    case 'standard':
      return import('./standard');
  }
}
