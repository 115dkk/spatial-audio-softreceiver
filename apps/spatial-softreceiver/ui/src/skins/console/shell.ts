// The console's window: the sidebar (pages, the open page's sections, the
// engine line), the top bar (title, the palette's trigger, the power button),
// and in the content the health line first (law 1), the error bands and
// notes under it, then the page. What the controls do is the shared
// controller's (src/skins/controller.ts); this is only the drawing, plus the
// skin's own view state: the palette, the narrow window's menu and the
// 60-second traces, all dropped on reload (ADR 0001, ADR 0041).

import { h, svg } from '../../dom';
import { bannerAction } from '../../screens/banner';
import type { Drawn, Live, Tab } from '../../screens/common';
import { powerState } from '../../screens/nav';
import { text } from '../../text';
import type { Frame } from '../controller';
import { type Health, health } from './health';
import type { HistorySelection } from './history';
import { type Build, type Section, act, build, cls, command, plain, toneGlyph } from './kit';
import { type Page, overview, traceValues } from './overview';
import { Palette, searchGlyph } from './palette';
import { profilePage } from './profile';
import { settingsPage } from './settings';
import { setupPage } from './setup';
import { Traces } from './trace';
import { upmixPage } from './upmix';

const TABS: readonly Tab[] = ['status', 'profile', 'upmix', 'settings'];

const pageName = (tab: Tab): string =>
  tab === 'status' ? text.console.overview : tab === 'profile' ? text.profile.title : tab === 'upmix' ? text.upmix.title : text.settings.title;

/** Stroked 16 px glyphs for the pages; currentColor, no icon font. */
const PAGE_ICONS: Record<Tab, string> = {
  status: 'M2 9h3l2-5 2.5 9 2-6 1 2H15',
  profile: 'M3 10V8a5 5 0 0 1 10 0v2M3 10h2v4H3zM11 10h2v4h-2z',
  upmix: 'M8 2v12M4 5v6M12 5v6M1 7v2M15 7v2',
  settings: 'M2 4h7M12 4h2M2 12h2M7 12h7M10 2v4M5 10v4',
};

function icon(d: string): SVGSVGElement {
  return svg(
    'svg',
    { class: 'cn-icon', viewBox: '0 0 16 16', width: 16, height: 16, 'aria-hidden': 'true', focusable: 'false' },
    svg('path', { d, fill: 'none', stroke: 'currentColor', 'stroke-width': 1.5, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }),
  );
}

/** The product mark, the shape the app icon uses, in ink. */
function brandMark(): SVGSVGElement {
  return svg(
    'svg',
    { class: 'cn-brand-mark', viewBox: '0 0 1024 1024', width: 24, height: 24, 'aria-hidden': 'true', focusable: 'false' },
    svg('rect', { width: 1024, height: 1024, rx: 200, class: 'cn-brand-tile' }),
    svg(
      'g',
      { class: 'cn-brand-glyph' },
      svg('path', { d: 'M349 272H285L180 352V672L285 752H349', fill: 'none', 'stroke-width': 82, 'stroke-linejoin': 'round' }),
      svg('path', { d: 'M675 272H739L844 352V672L739 752H675', fill: 'none', 'stroke-width': 82, 'stroke-linejoin': 'round' }),
      svg('rect', { x: 338, y: 428, width: 80, height: 168, rx: 40 }),
      svg('rect', { x: 466, y: 352, width: 92, height: 320, rx: 46 }),
      svg('rect', { x: 606, y: 428, width: 80, height: 168, rx: 40 }),
    ),
  );
}

const toneOfPower = { on: 'good', warning: 'warn', error: 'crit', off: 'off' } as const;

export interface ConsoleSkin {
  render(frame: Frame): Drawn;
}

export function createConsole(root: HTMLElement): ConsoleSkin {
  const traces = new Traces();
  const palette = new Palette(root);
  let menuOpen = false;
  let wasOutputsOpen = false;
  let historySelection: HistorySelection | null = null;
  const selection = { get: () => historySelection, set: (next: HistorySelection) => (historySelection = next) };
  /** The output list was opened from a band or the health line, away from its toggle. */
  let revealOutputs = false;
  let last: { frame: Frame } | null = null;

  const redraw = () => last?.frame.ctx.setView({});
  const focusKey = (key: string) => root.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`)?.focus();

  function setMenu(open: boolean, focus: boolean) {
    if (menuOpen === open) return;
    menuOpen = open;
    redraw();
    if (!focus) return;
    if (open) root.querySelector<HTMLElement>('.cn-side [aria-current="page"], .cn-side button, .cn-side a')?.focus();
    else focusKey('cn-menu');
  }

  function jump(id: string) {
    const target = document.getElementById(id);
    if (target === null) return;
    const still = window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ?? true;
    target.scrollIntoView?.({ block: 'start', behavior: still ? 'auto' : 'smooth' });
    target.querySelector<HTMLElement>('.cn-section-title')?.focus({ preventScroll: true });
  }

  function goPage(tab: Tab) {
    menuOpen = false;
    last?.frame.ctx.setView({ tab });
    window.scrollTo?.(0, 0);
  }

  function go(tab: Tab, section: string | null) {
    goPage(tab);
    if (section !== null) jump(section);
  }

  function chooseOutput(frame: Frame) {
    revealOutputs = true;
    frame.chooseOutput();
  }

  function onKey(event: KeyboardEvent) {
    if (!root.isConnected) {
      document.removeEventListener('keydown', onKey);
      return;
    }
    const phase = last?.frame.ctx.snapshot.phase.kind;
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 'k') {
      if (phase === undefined || phase === 'quit') return;
      event.preventDefault();
      if (palette.isOpen) palette.close();
      else palette.open();
      return;
    }
    if (event.key === 'Escape' && menuOpen && !palette.isOpen) {
      event.preventDefault();
      setMenu(false, true);
    }
  }
  document.addEventListener('keydown', onKey);

  // ---------------------------------------------------------------------------

  function sidebar(frame: Frame, sections: Section[]): HTMLElement {
    const { ctx } = frame;
    const { snapshot, view } = ctx;
    const phase = snapshot.phase.kind;
    const list = h('ul', { class: 'cn-nav' });
    const subnav = () =>
      sections.length === 0
        ? null
        : h(
            'ul',
            { class: 'cn-subnav', 'aria-label': text.console.sectionsLabel },
            ...sections.map((section) =>
              h(
                'li',
                null,
                h(
                  'a',
                  {
                    href: `#${section.id}`,
                    class: 'cn-subnav-item',
                    'data-key': `nav-${section.id}`,
                    onClick: (event: Event) => {
                      event.preventDefault();
                      if (menuOpen) setMenu(false, false);
                      jump(section.id);
                    },
                  },
                  section.label,
                ),
              ),
            ),
          );
    if (phase === 'setup') {
      list.append(
        h(
          'li',
          null,
          h('span', { class: 'cn-nav-item is-static', 'aria-current': 'page' }, icon(PAGE_ICONS.settings), h('span', null, text.wizard.heading)),
          subnav(),
        ),
      );
    } else if (phase !== 'quit') {
      for (const tab of TABS) {
        const current = view.tab === tab;
        list.append(
          h(
            'li',
            null,
            h(
              'button',
              {
                type: 'button',
                class: 'cn-nav-item',
                'data-key': `nav-${tab}`,
                'aria-current': current ? 'page' : null,
                onClick: () => goPage(tab),
              },
              icon(PAGE_ICONS[tab]),
              h('span', null, pageName(tab)),
            ),
            current ? subnav() : null,
          ),
        );
      }
    }
    const power = powerState(snapshot, ctx.pending);
    const tone = toneOfPower[power.tone];
    return h(
      'aside',
      { class: 'cn-side', id: 'cn-side' },
      h('div', { class: 'cn-brand' }, brandMark(), h('span', { class: 'cn-brand-name' }, text.product)),
      list.childElementCount === 0 ? null : h('nav', { class: 'cn-side-nav', 'aria-label': text.nav.label }, list),
      h(
        'div',
        { class: `cn-engine tone-${tone}` },
        h('span', { class: 'cn-engine-label' }, text.console.engine),
        h('span', { class: 'cn-engine-state' }, h('span', { class: 'cn-lamp', 'aria-hidden': 'true' }), power.label),
        h('span', { class: 'cn-engine-device cn-ellipsis', title: snapshot.chosen_output?.name ?? null }, snapshot.chosen_output?.name ?? text.status.noOutput),
        h(
          'span',
          { class: 'cn-engine-host', 'data-region': 'computer' },
          h('span', { class: 'cn-engine-label' }, text.console.computer),
          h('span', { class: 'cn-ellipsis' }, snapshot.platform?.computer_name || text.settings.platform),
        ),
      ),
    );
  }

  function powerButton(b: Build): HTMLButtonElement {
    const s = text.status;
    const running = b.ctx.snapshot.phase.kind === 'running';
    b.group = text.console.engine;
    const button = act(b, running ? s.powerOff : s.powerOn, running ? { kind: 'disable' } : { kind: 'enable' }, {
      key: 'power',
      variant: 'ink',
      spinner: true,
      pendingLabel: running ? s.powerTurningOff : s.powerTurningOn,
      className: 'cn-power',
    });
    b.group = null;
    return button;
  }

  function topbar(frame: Frame, title: string, power: HTMLButtonElement | null): HTMLElement {
    const phase = frame.ctx.snapshot.phase.kind;
    return h(
      'header',
      { class: 'cn-top' },
      h(
        'button',
        {
          type: 'button',
          class: 'cn-menu-button',
          'data-key': 'cn-menu',
          'aria-label': text.console.menu,
          'aria-expanded': menuOpen ? 'true' : 'false',
          'aria-controls': 'cn-side',
          onClick: () => setMenu(!menuOpen, true),
        },
        icon('M2 4h12M2 8h12M2 12h12'),
      ),
      h('h1', { class: 'cn-title' }, title),
      phase === 'quit'
        ? null
        : h(
            'button',
            {
              type: 'button',
              class: 'cn-palette-trigger',
              'data-key': 'cn-palette',
              'aria-haspopup': 'dialog',
              'aria-keyshortcuts': 'Control+K',
              onClick: () => palette.open(),
            },
            searchGlyph(),
            h('span', { class: 'cn-palette-trigger-label' }, text.console.paletteOpen),
            h('kbd', { class: 'cn-kbd', 'aria-hidden': 'true' }, 'Ctrl K'),
          ),
      power,
    );
  }

  function healthLine(b: Build, frame: Frame, state: Health): HTMLElement {
    let control: HTMLElement | null = null;
    const action = state.action;
    b.group = text.console.healthLabel;
    if (action !== null) {
      switch (action.kind) {
        case 'intent':
          // Offered elsewhere already (the power button): the line names it, the control stays one.
          if (!frame.offered.has(action.intent.kind)) control = act(b, action.label, action.intent, { key: 'health-action' });
          break;
        case 'choose_output':
          control = plain(b, action.label, () => chooseOutput(frame), { key: 'health-action' });
          break;
        case 'go':
          control = plain(b, action.label, () => go(action.tab, action.section), { key: 'health-action' });
          break;
      }
    }
    b.group = null;
    return h(
      'section',
      { class: `cn-health tone-${state.tone}`, 'data-region': 'health', 'aria-label': text.console.healthLabel },
      h('span', { class: 'cn-health-mark' }, toneGlyph(state.tone)),
      h(
        'div',
        { class: 'cn-health-text', role: 'status' },
        h('p', { class: 'cn-health-title' }, state.title),
        state.detail === null ? null : h('p', { class: 'cn-health-detail' }, state.detail),
      ),
      control === null ? null : h('div', { class: 'cn-health-action' }, control),
    );
  }

  function bands(b: Build, frame: Frame): HTMLElement | null {
    if (frame.errors.length === 0) return null;
    const { snapshot } = frame.ctx;
    const list = h('div', { class: 'cn-bands', 'aria-label': text.banner.errorLabel });
    frame.errors.forEach(({ error, kept }, index) => {
      let action = bannerAction(snapshot.phase, error.next);
      if (!kept && action.kind === 'nothing' && error.next === 'none') action = { kind: 'dismiss', label: text.banner.dismiss };
      b.group = text.banner.errorLabel;
      const key = `banner-${index}`;
      let control: HTMLElement | null = null;
      switch (action.kind) {
        case 'intent':
          if (!frame.offered.has(action.intent.kind)) control = act(b, action.label, action.intent, { key });
          break;
        case 'choose_output':
          control = plain(b, action.label, () => chooseOutput(frame), { key });
          break;
        case 'dismiss':
          control = plain(b, action.label, () => frame.dismiss(error), { key, variant: 'quiet' });
          break;
        case 'nothing':
          break;
      }
      b.group = null;
      const tone = error.next === 'none' && snapshot.phase.kind === 'running' ? 'warn' : 'crit';
      list.append(
        h(
          'div',
          { class: `cn-band tone-${tone}`, role: 'alert', 'data-region': 'error-band' },
          h('span', { class: 'cn-band-mark' }, toneGlyph(tone)),
          h(
            'div',
            { class: 'cn-band-text' },
            h('p', { class: 'cn-band-message' }, error.message),
            error.next === 'reboot' ? h('p', { class: 'cn-band-hint' }, text.banner.rebootHint) : null,
          ),
          control === null ? null : h('div', { class: 'cn-band-action' }, control),
        ),
      );
    });
    return list;
  }

  function notes(lines: string[]): HTMLElement | null {
    if (lines.length === 0) return null;
    return h('ul', { class: 'cn-notes', 'aria-label': text.banner.notesLabel }, ...lines.map((line) => h('li', null, line)));
  }

  function goCommands(b: Build, frame: Frame, sections: Section[]) {
    const { view, snapshot } = frame.ctx;
    const c = text.console;
    const phase = snapshot.phase.kind;
    if (phase === 'ready' || phase === 'running' || phase === 'faulted') {
      for (const tab of TABS) {
        command(b, {
          id: `go-${tab}`,
          label: c.goTo(pageName(tab)),
          group: text.nav.label,
          reason: view.tab === tab ? c.current : null,
          run: () => goPage(tab),
        });
      }
    }
    const here = phase === 'setup' ? c.checkTitle : pageName(view.tab);
    for (const section of sections) {
      command(b, { id: `go-${section.id}`, label: c.goTo(section.label), group: here, reason: null, run: () => jump(section.id) });
    }
  }

  function render(frame: Frame): Drawn {
    const { ctx, engine } = frame;
    const { snapshot, view } = ctx;
    const phase = snapshot.phase;
    if (phase.kind !== 'running') traces.clear();
    const b = build(ctx);

    let title: string;
    let page: Page | null = null;
    let power: HTMLButtonElement | null = null;
    if (phase.kind === 'setup') {
      title = text.console.checkTitle;
      page = setupPage(b, phase.value);
    } else if (phase.kind === 'quit') {
      title = text.product;
    } else {
      title = pageName(view.tab);
      power = powerButton(b);
      page =
        view.tab === 'status'
          ? overview(b, engine, traces, selection)
          : view.tab === 'profile'
            ? profilePage(b, engine)
            : view.tab === 'upmix'
              ? upmixPage(b)
              : settingsPage(b);
    }
    const sections = [...b.sections];
    // The health line comes after the page in the drawing so it knows what the
    // page offers; the bands come after it so they do not repeat its action.
    const healthElement = healthLine(b, frame, health(frame));
    const bandElement = bands(b, frame);
    goCommands(b, frame, sections);
    palette.setCommands(b.commands);

    const content = h(
      'main',
      { class: 'cn-content', id: 'main' },
      healthElement,
      bandElement,
      notes(snapshot.notes),
      page?.element ?? null,
    );
    const element = h(
      'div',
      { class: cls('cn-app', menuOpen && 'is-menu-open'), 'data-phase': phase.kind, 'data-tab': phase.kind === 'setup' || phase.kind === 'quit' ? null : view.tab },
      sidebar(frame, sections),
      h('div', { class: 'cn-scrim', 'aria-hidden': 'true', onClick: () => setMenu(false, true) }),
      h('div', { class: 'cn-main' }, topbar(frame, title, power), content),
    );

    const now = performance.now();
    page?.traces?.(now);

    // The output list opened from a band or the health line: bring it into view.
    const outputsOpen = phase.kind === 'ready' && view.outputsOpen;
    if (outputsOpen && !wasOutputsOpen && revealOutputs) {
      requestAnimationFrame(() => root.querySelector('#cn-output-list')?.scrollIntoView?.({ block: 'nearest' }));
    }
    if (outputsOpen !== wasOutputsOpen) revealOutputs = false;
    wasOutputsOpen = outputsOpen;

    last = { frame };
    if (phase.kind !== 'running' || page === null) return { element };
    const shown = page;
    const live: Live = {
      update(next) {
        const at = performance.now();
        if (traces.sample(at, traceValues(next))) shown.traces?.(at);
        shown.fill(next);
      },
    };
    return { element, live };
  }

  return { render };
}
