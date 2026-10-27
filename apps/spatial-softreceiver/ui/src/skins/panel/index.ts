// The front panel skin (ADR 0041, 전면 패널의 법칙): the window is one
// machined AV receiver chassis. The parts a real receiver has on its front
// sit on the chassis (src/skins/panel/chassis.ts); everything else is inside
// the black, self-lit display set into it, in the display's own grammar
// (src/skins/panel/ui.ts). What the controls do is the controller every skin
// shares (src/skins/controller.ts); this file only builds the window from a
// frame.

import './panel.css';
import type { Api } from '../../api';
import { h, liveText, setAttr, svg } from '../../dom';
import { describeOutputFormat } from '../../format';
import { text } from '../../text';
import type { Drawn, Live, ViewState } from '../../screens/common';
import { stepIndex } from '../../screens/wizard';
import { type Frame, mountWith } from '../controller';
import type { App, AppOptions } from '../types';
import { type EncoderHold, encoder, outputKeys, pageKeys, powerButton, powerLamp, powerWord, profileOpen, signalLamps } from './chassis';
import { profilePage } from './profile';
import type { Scope } from './scope';
import { settingsPage } from './settings';
import { type PageDrawn, statusPage } from './status';
import { band, kindIcon, notes } from './ui';
import { upmixPage } from './upmix';
import { followRatio } from './type';
import { quitPage, wizardPage } from './wizard';

/** How long a line stays on the OSD. */
const OSD_MS = 1600;

/** Windows at least this tall dock the health strip at the display's foot. */
const TALL = '(min-height: 760px)';

/** A dot-matrix arrow, up or down, in the display's dim white. */
function arrow(up: boolean): SVGSVGElement {
  // Three rows of square pixels: 1, 3 and 5 wide.
  const rows = up ? [[2], [1, 2, 3], [0, 1, 2, 3, 4]] : [[0, 1, 2, 3, 4], [1, 2, 3], [2]];
  return svg(
    'svg',
    { viewBox: '0 0 10 6', width: 10, height: 6, focusable: 'false', 'aria-hidden': 'true' },
    ...rows.flatMap((columns, row) => columns.map((column) => svg('rect', { x: column * 2, y: row * 2, width: 1.6, height: 1.6 }))),
  );
}

export function mount(root: HTMLElement, api: Api, options: AppOptions = {}): App {
  // The display's pixel type stays on the device's pixel grid at any scaling.
  followRatio(root);
  // Seen only, never kept: the encoder's turn and the OSD's line and timer.
  const hold: EncoderHold = { drag: null, wheel: null };
  let osdLine: { line: string; until: number } | null = null;
  let osdElement: HTMLElement | null = null;
  let osdTimer: ReturnType<typeof setTimeout> | null = null;
  // The display's page scroll, kept across redraws of the same page.
  let scrolled: { page: string; top: number } | null = null;
  let watching: ResizeObserver | null = null;
  // A window that grows or shrinks past TALL docks or frees the strip.
  let redraw: ((patch: Partial<ViewState>) => void) | null = null;
  const tall = typeof window.matchMedia === 'function' ? window.matchMedia(TALL) : null;
  tall?.addEventListener?.('change', () => redraw?.({}));

  function showOsd(line: string) {
    osdLine = { line, until: Date.now() + OSD_MS };
    if (osdElement !== null) {
      osdElement.textContent = line;
      osdElement.hidden = false;
    }
    if (osdTimer !== null) clearTimeout(osdTimer);
    osdTimer = setTimeout(() => {
      osdLine = null;
      osdTimer = null;
      if (osdElement !== null) osdElement.hidden = true;
    }, OSD_MS);
  }

  function render(frame: Frame): Drawn {
    const { ctx } = frame;
    const { snapshot, view } = ctx;
    redraw = ctx.setView;
    const phase = snapshot.phase;
    const able = profileOpen(snapshot);
    const running = phase.kind === 'running';
    const engineNow = running ? frame.engine : null;

    const readouts: ((db: number) => void)[] = [];
    const settled = ctx.pending === 'set_gain' && view.gainSending !== null ? view.gainSending : snapshot.profile.gain_db;
    const scope: Scope = {
      ctx,
      frame,
      gain: { settled, value: () => hold.drag ?? settled, follow: (set) => readouts.push(set) },
      osd: showOsd,
    };

    // The chassis first: what its parts offer, the bands below need not repeat.
    const power = powerButton(ctx);
    const lamps = signalLamps();
    lamps.update(engineNow);
    const knob = encoder(scope, hold, (db) => {
      for (const set of readouts) set(db);
    });
    const kinds = outputKeys(scope);
    const pages = pageKeys(ctx, able);

    let page: PageDrawn;
    let title: string | null = null;
    if (phase.kind === 'setup') page = wizardPage(ctx, phase.value);
    else if (phase.kind === 'quit') page = quitPage();
    else {
      switch (view.tab) {
        case 'status':
          page = statusPage(scope, engineNow, tall?.matches === true);
          break;
        case 'profile':
          page = profilePage(scope, engineNow);
          break;
        case 'upmix':
          page = upmixPage(scope);
          break;
        case 'settings':
          page = settingsPage(scope);
          break;
      }
      if (view.tab !== 'status') title = text.nav[view.tab];
    }

    // The bands after everything that offers an intent. The snapshot's stand
    // under the status line; a refused intent's at the foot of the display.
    const kept: HTMLElement[] = [];
    const refused: HTMLElement[] = [];
    frame.errors.forEach((entry, index) => (entry.kept ? kept : refused).push(band(ctx, frame, entry, index)));

    // The status line: the power state, the page, and what plays.
    const lampNow = powerLamp(snapshot, ctx.pending, frame.engine);
    const word = liveText(phase.kind === 'setup' ? text.wizard.heading : phase.kind === 'quit' ? text.product : powerWord(lampNow, snapshot));
    const lampDot = h('span', { class: 'pn-dot', 'data-lamp': phase.kind === 'setup' || phase.kind === 'quit' ? 'off' : lampNow, 'aria-hidden': 'true' });
    const format = liveText(engineNow === null ? '' : describeOutputFormat(engineNow));
    const meta =
      phase.kind === 'setup'
        ? h('span', { class: 'pn-meta num' }, `${text.panel.step} ${stepIndex(phase.value) + 1}/${text.wizard.steps.length}`)
        : able
          ? h(
              'span',
              { class: 'pn-meta' },
              running ? h('span', { class: 'num pn-meta-format' }, format.node) : null,
              h('span', { class: 'pn-meta-kind' }, kindIcon(snapshot.profile.kind), text.profile.kindNames[snapshot.profile.kind]),
            )
          : null;
    const statusLine = h(
      'div',
      { class: 'pn-statusline' },
      h('span', { class: 'pn-state' }, lampDot, h('span', { id: 'pn-state-word', class: 'pn-state-word' }, word.node)),
      title === null ? null : h('span', { class: 'pn-page-title' }, title),
      meta,
    );

    const osd = h('div', { class: 'pn-osd', 'aria-hidden': 'true', hidden: osdLine === null || osdLine.until <= Date.now() }, osdLine?.line ?? '');
    osdElement = osd;

    const tabbed = able;
    const pageName = tabbed ? view.tab : phase.kind;
    const scroller = h(
      'div',
      {
        class: 'pn-page',
        id: 'pn-page',
        role: tabbed ? 'tabpanel' : null,
        'aria-labelledby': tabbed ? `pn-tab-${view.tab}` : null,
        tabindex: 0,
        'data-page': pageName,
      },
      page.element,
    );
    // More above or below: a dot-matrix arrow at the display's right edge.
    const above = h('span', { class: 'pn-cue is-up', 'aria-hidden': 'true', hidden: true }, arrow(true));
    const below = h('span', { class: 'pn-cue is-down', 'aria-hidden': 'true', hidden: true }, arrow(false));
    const cue = () => {
      const rest = scroller.scrollHeight - scroller.clientHeight - scroller.scrollTop;
      above.hidden = scroller.scrollTop <= 1;
      below.hidden = rest <= 1;
      scrolled = { page: pageName, top: scroller.scrollTop };
    };
    scroller.addEventListener('scroll', cue, { passive: true });
    const keep = scrolled !== null && scrolled.page === pageName ? scrolled.top : 0;
    watching?.disconnect();
    watching = typeof ResizeObserver === 'function' ? new ResizeObserver(cue) : null;
    // Once the frame is in the document: back to where the page was, then the arrows.
    queueMicrotask(() => {
      if (!scroller.isConnected) return;
      if (keep > 0) scroller.scrollTop = keep;
      cue();
      watching?.observe(scroller);
      watching?.observe(page.element);
    });

    const screen = h(
      'div',
      { class: 'pn-screen' },
      statusLine,
      kept.length === 0 && snapshot.notes.length === 0
        ? null
        : h(
            'div',
            { class: 'pn-bands', 'aria-live': 'polite' },
            kept.length === 0 ? null : h('div', { 'aria-label': text.banner.errorLabel, role: 'group' }, ...kept),
            notes(snapshot.notes),
          ),
      h('div', { class: 'pn-pagewrap' }, scroller, above, below),
      page.foot ?? null,
      refused.length === 0 ? null : h('div', { class: 'pn-refusals' }, ...refused),
      osd,
    );

    const element = h(
      'div',
      { class: 'pn-room', 'data-phase': phase.kind },
      h(
        'div',
        { class: 'pn-chassis' },
        h('div', { class: 'pn-plate', 'aria-hidden': 'true' }, h('span', { class: 'pn-print pn-brand' }, text.product)),
        h('div', { class: 'pn-left' }, power.element, lamps.element),
        h(
          'main',
          { class: 'pn-centre' },
          h('div', { class: 'pn-display' }, screen),
          pages,
        ),
        h('div', { class: 'pn-right' }, knob, kinds),
      ),
    );

    if (!running) return { element };
    const live: Live = {
      update(engine) {
        lamps.update(engine);
        power.update(engine);
        const lamp = powerLamp(snapshot, ctx.pending, engine);
        setAttr(lampDot, 'data-lamp', lamp);
        word.set(powerWord(lamp, snapshot));
        format.set(describeOutputFormat(engine));
        page.live?.update(engine);
      },
    };
    return { element, live };
  }

  return mountWith(root, api, options, { render });
}
