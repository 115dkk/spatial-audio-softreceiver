// The playground's own layer over the console's drawing (ADR 0047): the tip
// strip, the workspace and account that do not exist, the PRO chips, the
// begging card, the toasts and the three modals that ask for alms, a star or
// a rating. Nothing here decides or keeps anything the product needs: every
// intent has already gone to the supervisor by the time a modal opens, and
// the only thing remembered is which features have asked once (per viewer,
// in localStorage, so a missing store only means being asked again).

import type { Api } from '../../api';
import type { Intent } from '../../bindings';
import { type Child, h } from '../../dom';
import type { Tab } from '../../screens/common';
import type { Frame } from '../controller';
import { type Catalog, en, ko } from './catalog';

type Own = Catalog['own'];
type FeatureKey = keyof Own['features'];

/** The features that are "PRO": using one begs instead of asking for a star. Free all the same. */
const PRO_INTENTS: ReadonlySet<Intent['kind']> = new Set<Intent['kind']>([
  'set_profile_kind',
  'set_gain',
  'set_upmixer',
  'set_speaker_layout',
  'choose_output',
  'start_diagnostic_recording',
]);
const PRO_TABS: ReadonlySet<Tab> = new Set<Tab>(['upmix']);
/** Controls that wear a PRO chip. */
const PRO_PARTS = ['[data-key="nav-upmix"]', '[data-key="profile-tab-speakers"]', '[data-key="record"]', '[data-key="outputs-toggle"]'];
/** Intents that end the page or the app: nothing is asked after them. */
const SILENT: ReadonlySet<Intent['kind']> = new Set<Intent['kind']>(['set_skin', 'quit']);

const ASKED_KEY = 'ssr.playground.asked';
const TOAST_MS = 3600;

type ModalKind = 'beg' | 'yesno' | 'stars';
interface Asking {
  kind: ModalKind;
  feature: string;
}

export interface Hype {
  /** An intent the supervisor took: a toast, and once per feature a modal. */
  used(intent: Intent): void;
  /** Adds the playground's parts to a frame the console drew. */
  decorate(element: HTMLElement, frame: Frame): void;
}

function loadAsked(): Set<string> {
  try {
    const stored: unknown = JSON.parse(localStorage.getItem(ASKED_KEY) ?? '[]');
    return new Set(Array.isArray(stored) ? stored.filter((item): item is string => typeof item === 'string') : []);
  } catch {
    return new Set();
  }
}

function saveAsked(asked: Set<string>): void {
  try {
    localStorage.setItem(ASKED_KEY, JSON.stringify([...asked]));
  } catch {
    // Without a store the features only ask again after a reload.
  }
}

/** Random tips; the gallery's pictures seed it from the scene so each is the same every run. */
function dice(): () => number {
  const params = new URLSearchParams(location.search);
  if (!params.has('gallery')) return Math.random;
  let state = 0x9e3779b9;
  for (const char of `${params.get('scene') ?? ''}${params.get('lang') ?? ''}`) state = Math.imul(state ^ char.charCodeAt(0), 0x01000193) >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export function createHype(root: HTMLElement, api: Api): Hype {
  const random = dice();
  const asked = loadAsked();
  let own: Own = ko.own;
  let asks = 0;
  let asking: Asking | null = null;
  let returnTo: HTMLElement | null = null;
  let tip = Math.floor(random() * ko.own.tips.length);
  let lastTab: Tab | null = null;
  let redraw: (() => void) | null = null;

  // The toasts and the modal live beside the app, so a redraw does not take them.
  const toasts = h('div', { class: 'pg-toasts', role: 'status', 'aria-live': 'polite' });
  const modalHost = h('div', { class: 'pg-modal-host' });
  const layer = h('div', { class: 'pg-layer' }, toasts, modalHost);
  (root.parentElement ?? document.body).append(layer);

  const open = (link: 'repo' | 'sponsors') => {
    api.shell.openLink(link).catch(() => undefined);
  };

  function toast(title: string, body: string): void {
    const item = h('div', { class: 'pg-toast' }, h('b', null, title), h('span', null, body));
    toasts.append(item);
    while (toasts.childElementCount > 3) toasts.firstElementChild?.remove();
    setTimeout(() => item.remove(), TOAST_MS);
  }

  function nextTip(): void {
    const total = own.tips.length;
    // Never the same tip twice in a row.
    tip = (tip + 1 + Math.floor(random() * (total - 1))) % total;
  }

  function closeModal(): void {
    asking = null;
    modalHost.replaceChildren();
    const back = returnTo;
    returnTo = null;
    if (back?.isConnected) back.focus();
    else root.querySelector<HTMLElement>('#main')?.focus();
  }

  function answer(run: () => void): () => void {
    return () => {
      closeModal();
      run();
    };
  }

  function drawModal(): void {
    if (asking === null) return;
    const m = own.modal;
    const t = own.toast;
    const { kind, feature } = asking;
    const button = (label: string, className: string, onClick: () => void) => h('button', { type: 'button', class: `pg-button ${className}`, onClick }, label);
    let body: Child[];
    if (kind === 'beg') {
      body = [
        h('div', { class: 'pg-modal-emoji', 'aria-hidden': 'true' }, '🥺'),
        h('h2', { id: 'pg-modal-title' }, m.begTitle),
        h('p', null, m.begLead(feature)),
        h('p', null, m.begStory),
        h(
          'div',
          { class: 'pg-modal-actions' },
          button(
            m.give,
            'is-hype',
            answer(() => {
              open('sponsors');
              toast(t.giveTitle, t.giveBody);
            }),
          ),
          button(
            m.starve,
            '',
            answer(() => toast(t.starveTitle, t.starveBody(feature))),
          ),
        ),
        h('p', { class: 'pg-modal-foot' }, m.begFoot(feature)),
      ];
    } else if (kind === 'yesno') {
      body = [
        h('div', { class: 'pg-modal-emoji', 'aria-hidden': 'true' }, '⭐'),
        h('h2', { id: 'pg-modal-title' }, m.yesnoTitle(feature)),
        h('p', null, m.yesnoBody),
        h(
          'div',
          { class: 'pg-modal-actions' },
          button(
            m.yes,
            'is-hype',
            answer(() => {
              open('repo');
              toast(t.yesTitle, t.yesBody);
            }),
          ),
          button(
            m.no,
            '',
            answer(() => toast(t.noTitle, t.noBody)),
          ),
        ),
      ];
    } else {
      const stars = [1, 2, 3, 4, 5].map((n) =>
        h(
          'button',
          {
            type: 'button',
            class: 'pg-star',
            'aria-label': m.star(n, m.grades[n - 1] ?? ''),
            onClick: answer(() => {
              open('repo');
              toast(t.ratedTitle(n), t.ratedBody);
            }),
          },
          '★',
        ),
      );
      const light = (upTo: number) => stars.forEach((star, index) => star.classList.toggle('is-lit', index < upTo));
      stars.forEach((star, index) => {
        star.addEventListener('pointerenter', () => light(index + 1));
        star.addEventListener('focus', () => light(index + 1));
      });
      body = [
        h('div', { class: 'pg-modal-emoji', 'aria-hidden': 'true' }, '🌟'),
        h('h2', { id: 'pg-modal-title' }, m.starsTitle(feature)),
        h('p', null, m.starsBody),
        h('div', { class: 'pg-stars', role: 'group', 'aria-label': m.starsLabel, onPointerleave: () => light(0) }, ...stars),
        h('div', { class: 'pg-stars-cap', 'aria-hidden': 'true' }, h('span', null, m.grades[0] ?? ''), h('span', null, m.grades[4] ?? '')),
        h('div', { class: 'pg-modal-actions is-one' }, button(m.later, 'is-ghost', closeModal)),
      ];
    }
    const dialog = h(
      'div',
      {
        class: 'pg-modal',
        role: 'dialog',
        'aria-modal': 'true',
        'aria-labelledby': 'pg-modal-title',
        'data-kind': kind,
        onKeydown: (event: Event) => {
          const key = event as KeyboardEvent;
          if (key.key === 'Escape') {
            key.preventDefault();
            closeModal();
            return;
          }
          if (key.key !== 'Tab') return;
          // The dialog keeps the focus while it is open.
          const parts = [...dialog.querySelectorAll<HTMLElement>('button')];
          const first = parts[0];
          const last = parts[parts.length - 1];
          if (first === undefined || last === undefined) return;
          if (key.shiftKey && document.activeElement === first) {
            key.preventDefault();
            last.focus();
          } else if (!key.shiftKey && document.activeElement === last) {
            key.preventDefault();
            first.focus();
          }
        },
      },
      h('button', { type: 'button', class: 'pg-modal-x', 'aria-label': m.close, onClick: closeModal }, '✕'),
      ...body,
    );
    modalHost.replaceChildren(h('div', { class: 'pg-scrim', onClick: closeModal }), dialog);
  }

  /** Asks once per feature, one modal at a time; PRO features beg, the others take turns asking for a star and a rating. */
  function ask(key: string, feature: string, pro: boolean): void {
    if (asking !== null || asked.has(key)) return;
    asked.add(key);
    saveAsked(asked);
    const kind: ModalKind = pro ? 'beg' : asks++ % 2 === 0 ? 'yesno' : 'stars';
    asking = { kind, feature };
    const active = document.activeElement;
    returnTo = active instanceof HTMLElement && active !== document.body ? active : null;
    drawModal();
    modalHost.querySelector<HTMLElement>('.pg-modal-actions button')?.focus();
  }

  const featureName = (key: string): string => (key in own.features ? own.features[key as FeatureKey] : own.features.other);

  function used(intent: Intent): void {
    if (SILENT.has(intent.kind)) return;
    const feature = featureName(intent.kind);
    toast(own.toast.doneTitle(feature), own.toast.doneBody);
    ask(intent.kind, feature, PRO_INTENTS.has(intent.kind));
  }

  function tipStrip(): HTMLElement {
    const total = own.tips.length;
    return h(
      'aside',
      { class: 'pg-tip', 'aria-label': own.tipLabel, 'data-region': 'tip' },
      h('span', { class: 'pg-tip-badge' }, own.tipBadge),
      h('span', { class: 'pg-tip-text' }, own.tips[tip % total] ?? ''),
      h('span', { class: 'pg-tip-count', 'aria-hidden': 'true' }, own.tipCount((tip % total) + 1, total)),
      h(
        'button',
        {
          type: 'button',
          class: 'pg-tip-next',
          'data-key': 'pg-tip-next',
          onClick: () => {
            nextTip();
            redraw?.();
          },
        },
        own.tipNext,
      ),
    );
  }

  function decorate(element: HTMLElement, frame: Frame): void {
    const { snapshot, view } = frame.ctx;
    own = snapshot.language === 'en' ? en.own : ko.own;
    redraw = () => frame.ctx.setView({});
    element.classList.add('pg-app');
    const phase = snapshot.phase.kind;
    const main = phase === 'ready' || phase === 'running' || phase === 'faulted';

    // Another page: another tip, and the page asks for something once.
    if (main) {
      if (lastTab !== null && lastTab !== view.tab) {
        nextTip();
        const key = `nav_${view.tab}`;
        ask(key, featureName(key), PRO_TABS.has(view.tab));
      }
      lastTab = view.tab;
    }
    if (asking !== null) drawModal();
    if (phase === 'quit') return;

    const brand = element.querySelector('.cn-brand');
    brand?.append(h('span', { class: 'pg-chip' }, own.brandChip));
    const w = own.workspace;
    brand?.after(
      h(
        'button',
        { type: 'button', class: 'pg-ws', 'data-key': 'pg-workspace', 'aria-label': w.label, onClick: () => toast(w.toastTitle, w.toastBody) },
        h('span', { class: 'pg-ws-mark', 'aria-hidden': 'true' }, [...w.name][0] ?? ''),
        h('span', { class: 'pg-ws-text' }, h('b', null, w.name), h('small', null, w.kind)),
        h('span', { class: 'pg-ws-caret', 'aria-hidden': 'true' }, '⌄'),
      ),
    );
    const beg = own.beg;
    element.querySelector('.cn-engine')?.before(
      h(
        'section',
        { class: 'pg-beg', 'aria-label': beg.title },
        h('b', null, beg.title),
        h('p', null, beg.body),
        h('div', { class: 'pg-beg-meter', role: 'img', 'aria-label': `${beg.meter} 23%` }, h('i')),
        h('button', { type: 'button', class: 'pg-beg-action', 'data-key': 'pg-beg', onClick: () => open('sponsors') }, beg.action),
      ),
    );

    const top = element.querySelector('.cn-top');
    const extras = [
      h('span', { class: 'pg-env' }, h('i', { 'aria-hidden': 'true' }), own.env),
      h(
        'button',
        {
          type: 'button',
          class: 'pg-avatar',
          'data-key': 'pg-account',
          'aria-label': own.avatarLabel,
          onClick: () => toast(own.account.toastTitle, own.account.toastBody),
        },
        own.avatar,
      ),
    ];
    const power = top?.querySelector('.cn-power');
    if (power != null) power.before(...extras);
    else top?.append(...extras);
    top?.after(tipStrip());

    for (const selector of PRO_PARTS) {
      element.querySelector(selector)?.append(h('span', { class: 'pg-pro' }, own.pro));
    }
    element.querySelector('.cn-content')?.append(h('p', { class: 'pg-footnote' }, own.footnote));
  }

  return { used, decorate };
}
