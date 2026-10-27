// The standard skin's drawing: the bar with the four tabs across the top, then
// one readable column with the open tab's title, the error bands and notes,
// and its page. What the controls do (one intent at a time, the settle wait,
// the bands' lifetime, focus across redraws) is the controller every skin
// shares (src/skins/controller.ts, ADR 0041); the view state is only which
// tab is open and the like (ADR 0001).

import type { Api } from './api';
import { h } from './dom';
import { text } from './text';
import { banners, notes } from './screens/banner';
import type { Drawn, Tab } from './screens/common';
import { nav } from './screens/nav';
import { profile } from './screens/profile';
import { settings } from './screens/settings';
import { status } from './screens/status';
import { upmix } from './screens/upmix';
import { quit, wizard } from './screens/wizard';
import { type Frame, mountWith } from './skins/controller';
import type { App, AppOptions } from './skins/types';

export type { App, AppOptions };

function render(frame: Frame): Drawn {
  const { ctx, engine } = frame;
  const { snapshot, view } = ctx;
  const phase = snapshot.phase;
  const wizardMode = phase.kind === 'setup' || phase.kind === 'quit';
  let head: HTMLElement;
  let body: HTMLElement;
  let live: Drawn['live'];
  if (phase.kind === 'setup') {
    ({ head, body } = wizard(ctx, phase.value));
  } else if (phase.kind === 'quit') {
    ({ head, body } = quit());
  } else {
    const titles: Record<Tab, string> = {
      status: text.status.title,
      profile: text.profile.title,
      upmix: text.upmix.title,
      settings: text.settings.title,
    };
    head = h('header', { class: 'main-head' }, h('h1', null, titles[view.tab]));
    const drawn =
      view.tab === 'status'
        ? status(ctx, engine)
        : view.tab === 'profile'
          ? profile(ctx, engine)
          : view.tab === 'upmix'
            ? upmix(ctx)
            : settings(ctx);
    body = drawn.element;
    live = drawn.live;
  }

  const alerts = h(
    'div',
    { class: 'alerts', 'aria-live': 'polite' },
    banners(ctx, {
      errors: frame.errors,
      offered: frame.offered,
      chooseOutput: frame.chooseOutput,
      dismiss: frame.dismiss,
    }),
    notes(snapshot.notes),
  );

  const main = h(
    'main',
    { class: 'main', id: 'main' },
    h('div', { class: 'page' }, head, alerts, h('div', { class: 'body' }, body)),
  );
  const element = h(
    'div',
    { class: `app ${wizardMode ? 'mode-wizard' : 'mode-main'}`, 'data-phase': phase.kind, 'data-tab': view.tab },
    wizardMode ? null : nav(ctx),
    main,
  );
  return live === undefined ? { element } : { element, live };
}

export function mountApp(root: HTMLElement, api: Api, options: AppOptions = {}): App {
  return mountWith(root, api, options, { render });
}
