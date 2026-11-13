// The dangerous playground skin (ADR 0047): the console's renderer under
// overblown SaaS copy, a parody. The drawing and what the controls do are the
// console's and the shared controller's, unchanged; this skin lays its own
// words over the catalog's (i18n/playground/*.toml, src/text.ts) and adds its
// own layer of parts on every frame (./hype.ts). Every feature works exactly
// as in the console, and nothing is paid for.

import '../console/console.css';
import './playground.css';
import type { Api } from '../../api';
import { setOverlay } from '../../text';
import { mountWith } from '../controller';
import { createConsole } from '../console/shell';
import type { App, AppOptions } from '../types';
import { en, ko } from './catalog';
import { createHype } from './hype';

export function mount(root: HTMLElement, api: Api, options: AppOptions = {}): App {
  setOverlay({ ko: ko.over, en: en.over });
  const skin = createConsole(root);
  const hype = createHype(root, api);
  // An intent the supervisor took is "used": the playground congratulates and asks.
  const watched: Api = {
    ...api,
    async dispatch(intent) {
      // Leaving for another skin is held on to first; staying sends nothing.
      if (intent.kind === 'set_skin' && intent.value !== 'playground' && !(await hype.leaving(intent.value))) return;
      await api.dispatch(intent);
      hype.used(intent);
    },
  };
  return mountWith(root, watched, options, {
    render(frame) {
      const drawn = skin.render(frame);
      hype.decorate(drawn.element, frame);
      return drawn;
    },
  });
}
