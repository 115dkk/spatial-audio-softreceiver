// The bar across the top: the mark, the four screens as tabs and, away from
// the status screen, the power state in one small word with its lamp. The
// status screen says the state in its own head, so the bar leaves it out
// there and the state is never said twice on one page.

import type { Intent, Snapshot } from '../bindings';
import { h } from '../dom';
import { text } from '../text';
import type { Context, Tab } from './common';

const TABS: Tab[] = ['status', 'profile', 'upmix', 'settings'];

/**
 * The power state in one word, and its tone. `pending` is the intent in
 * flight (`Context.pending`): while an `enable` is on its way the snapshot
 * still says off, but the screen knows it is turning on.
 */
export function powerState(
  snapshot: Snapshot,
  pending: Intent['kind'] | null,
): { label: string; tone: 'off' | 'on' | 'warning' | 'error' } {
  if (pending === 'enable') return { label: text.status.powerTurningOn, tone: 'warning' };
  switch (snapshot.phase.kind) {
    case 'running':
      return snapshot.engine?.fault
        ? { label: text.status.reconnecting, tone: 'warning' }
        : { label: text.status.on, tone: 'on' };
    case 'faulted':
      return { label: text.status.stopped, tone: 'error' };
    default:
      return { label: text.status.off, tone: 'off' };
  }
}

export function brand(): HTMLElement {
  return h(
    'div',
    { class: 'brand' },
    h('img', { class: 'brand-mark', src: '/mark.svg', alt: '', width: 24, height: 24 }),
    h('span', { class: 'brand-name' }, text.product),
  );
}

export function nav(ctx: Context): HTMLElement {
  const list = h('ul', { class: 'nav-list' });
  for (const tab of TABS) {
    const current = ctx.view.tab === tab;
    list.append(
      h(
        'li',
        null,
        h(
          'button',
          {
            type: 'button',
            class: 'nav-item',
            'data-key': `nav-${tab}`,
            'aria-current': current ? 'page' : null,
            onClick: () => ctx.setView({ tab }),
          },
          text.nav[tab],
        ),
      ),
    );
  }
  const power = powerState(ctx.snapshot, ctx.pending);
  return h(
    'header',
    { class: 'app-bar' },
    h(
      'div',
      { class: 'app-bar-inner' },
      brand(),
      h('nav', { class: 'nav', 'aria-label': text.nav.label }, list),
      ctx.view.tab === 'status'
        ? null
        : h(
            'p',
            { class: `nav-power tone-${power.tone}` },
            h('span', { class: 'lamp', 'aria-hidden': 'true' }),
            power.label,
          ),
    ),
  );
}
