// The enclosure (ADR 0046, 이펙터의 법칙 3, 4, 11): one cast box that fills
// the window. The input jacks and the output jack stand on its top edge in
// the signal's order, left to right; the face between the rails holds the
// open function's parts and runs on under the rails when the window is
// short, with the two arrow keys at its right; the foot carries the face
// keys, the nameplate, the three indicators and the power footswitch, the
// same on every face. A part that cannot act in this phase is off: dark and
// disabled.

import type { EngineStatus, Intent } from '../../bindings';
import { type Child, h, setAttr } from '../../dom';
import { en, text } from '../../text';
import type { Context, Tab } from '../../screens/common';
import {
  type Led,
  footswitchDrawing,
  ikey,
  jackDrawing,
  led,
  metalDefs,
  print,
  sr,
} from './parts';
import { ASIO_LANE, type Signal, brirInUse, powerLamp, powerWord, profileOpen, signals, upmixApplied } from './scope';

const TABS: readonly Tab[] = ['status', 'profile', 'upmix', 'settings'];
const JACKS = [0, 1, 2, ASIO_LANE];

export interface Part {
  element: HTMLElement;
  update(engine: EngineStatus | null): void;
}

/** The top edge: an input jack per entrance with its LED, the arrows of the flow, and the output jack. */
export function jackStrip(ctx: Context, lit: readonly number[]): Part {
  const p = text.pedal;
  const words: Record<Signal, string> = { sound: p.lampSound, idle: p.lampIdle, none: text.status.noInput };
  const jacks = JACKS.map((lane) => {
    const light = led(`lane-${lane}`, 'off', 'md');
    const said = sr(words.none);
    const element = h(
      'li',
      { class: 'pd-jack', 'data-lane': lane, 'data-signal': 'none' },
      jackDrawing(40),
      h('span', { class: 'pd-jack-text' }, h('span', { class: 'pd-name' }, p.lamps[lane] ?? String(lane), light.element), said),
    );
    return { element, light, said };
  });
  const device = ctx.snapshot.chosen_output?.name ?? text.status.noOutput;
  const element = h(
    'header',
    { class: 'pd-strip' },
    h(
      'div',
      { class: 'pd-cols pd-jacks' },
      h('ul', { class: 'pd-jack-list', 'aria-label': p.lampsLabel, 'aria-describedby': 'pd-jacks-hint' }, ...jacks.map((jack) => jack.element)),
      h(
        'div',
        { class: 'pd-jack is-out' },
        jackDrawing(40),
        h(
          'span',
          { class: 'pd-jack-text' },
          h('span', { class: 'pd-name' }, text.status.output),
          h('span', { class: `pd-small pd-jack-device${ctx.snapshot.chosen_output === null ? ' is-none' : ''}` }, device),
        ),
      ),
      sr(p.lampsHint, 'pd-jacks-hint'),
    ),
    h('div', { class: 'pd-rule', 'aria-hidden': 'true' }),
  );
  return {
    element,
    update(engine) {
      signals(engine, lit).forEach((signal, index) => {
        const jack = jacks[index];
        if (jack === undefined) return;
        setAttr(jack.element, 'data-signal', signal);
        jack.light.set(signal === 'sound' ? 'on' : signal === 'idle' ? 'dim' : 'off');
        if (jack.said.textContent !== words[signal]) jack.said.textContent = words[signal];
      });
    },
  };
}

/**
 * The power footswitch: enable while off or stopped, disable while running
 * (`data-key="power"`). Its name is the printed 전원 and what a press does;
 * the 켜짐 indicator beside it says the state.
 */
export function footswitch(ctx: Context): HTMLElement {
  const { snapshot } = ctx;
  const s = text.status;
  const running = snapshot.phase.kind === 'running';
  const able = profileOpen(snapshot);
  const intent: Intent = running ? { kind: 'disable' } : { kind: 'enable' };
  if (able) ctx.offer(intent.kind);
  const pending = ctx.pending === 'enable' || ctx.pending === 'disable';
  const action =
    ctx.pending === 'enable' ? s.powerTurningOn
    : ctx.pending === 'disable' ? s.powerTurningOff
    : running ? s.powerOff
    : s.powerOn;
  return h(
    'button',
    {
      type: 'button',
      class: `pd-footswitch${able ? '' : ' is-off'}`,
      'data-key': 'power',
      'data-intent': able ? intent.kind : null,
      disabled: !able || ctx.busy,
      'aria-busy': pending ? 'true' : null,
      'aria-labelledby': 'pd-power-print pd-power-action',
      'aria-describedby': 'pd-lamp-power-word',
      onClick: () => ctx.send(intent),
    },
    footswitchDrawing(72),
    h('span', { class: 'pd-small pd-key-label', id: 'pd-power-print' }, text.pedal.power),
    sr(action, 'pd-power-action'),
  );
}

/**
 * The indicators by the footswitch: 업믹스 while the output in use applies
 * an upmixer, BRIR while the headphones render through a BRIR set, and 켜짐
 * in the power lamp's colour. In setup and after quitting all three are dark.
 */
export function indicators(ctx: Context, engine: EngineStatus | null): Part {
  const { snapshot } = ctx;
  const able = profileOpen(snapshot);
  const p = text.pedal;
  const word = (on: boolean) => (on ? text.facts.on : text.facts.off);
  const lamp = (id: string, name: string, light: Led, said: HTMLElement) =>
    h('li', { class: 'pd-lamp', 'data-lamp': id }, print(name), light.element, said);

  const upmixOn = able && upmixApplied(snapshot);
  const brirOn = able && brirInUse(snapshot);
  const upmix = led('on', upmixOn ? 'on' : 'off', 'lg');
  const brir = led('on', brirOn ? 'on' : 'off', 'lg');
  const powerNow = able ? powerLamp(snapshot, ctx.pending, engine) : 'off';
  const power = led(powerNow === 'off' ? 'on' : powerNow, powerNow === 'off' ? 'off' : 'on', 'lg');
  const powerSaid = sr(able ? powerWord(powerNow, snapshot) : text.status.off, 'pd-lamp-power-word');
  const element = h(
    'ul',
    { class: 'pd-lamps', 'aria-label': p.indicators },
    lamp('upmix', p.upmixLamp, upmix, sr(word(upmixOn))),
    lamp('brir', p.brirLamp, brir, sr(word(brirOn))),
    lamp('power', text.status.on, power, powerSaid),
  );
  return {
    element,
    update(next) {
      if (!able) return;
      const now = powerLamp(snapshot, ctx.pending, next);
      power.set(now === 'off' ? 'off' : 'on', now === 'off' ? 'on' : now);
      const said = powerWord(now, snapshot);
      if (powerSaid.textContent !== said) powerSaid.textContent = said;
    },
  };
}

/**
 * The face keys: illuminated push buttons acting as tabs, their ring lit on
 * the face that is open. Choosing one only changes view state; in setup and
 * after quitting they are dark and off.
 */
export function faceKeys(ctx: Context, able: boolean): HTMLElement {
  const choose = (tab: Tab, focus: boolean) => {
    if (focus) list.querySelector<HTMLElement>(`[data-key="nav-${tab}"]`)?.focus();
    ctx.setView({ tab });
  };
  const list = h('div', {
    class: 'pd-facekeys',
    role: 'tablist',
    'aria-label': text.nav.label,
    onKeydown: (event: Event) => {
      if (!able) return;
      const key = (event as KeyboardEvent).key;
      const focused = (document.activeElement as HTMLElement | null)?.dataset.key?.replace('nav-', '');
      const at = TABS.indexOf(TABS.find((tab) => tab === focused) ?? ctx.view.tab);
      const to =
        key === 'ArrowRight' ? (at + 1) % TABS.length
        : key === 'ArrowLeft' ? (at - 1 + TABS.length) % TABS.length
        : key === 'Home' ? 0
        : key === 'End' ? TABS.length - 1
        : null;
      const tab = to === null ? undefined : TABS[to];
      if (tab === undefined) return;
      event.preventDefault();
      choose(tab, true);
    },
  });
  for (const tab of TABS) {
    const selected = able && ctx.view.tab === tab;
    list.append(
      ikey({
        key: `nav-${tab}`,
        label: text.nav[tab],
        lit: selected,
        chosen: selected,
        off: !able,
        attrs: {
          role: 'tab',
          id: `pd-tab-${tab}`,
          'aria-selected': selected ? 'true' : 'false',
          'aria-controls': able ? 'pd-face' : null,
          tabindex: selected ? 0 : -1,
        },
        onClick: () => choose(tab, false),
      }),
    );
  }
  return list;
}

/** The nameplate: a separate plate in the opposite finish, the product's name engraved. */
function nameplate(): HTMLElement {
  const brand = en.product;
  return h(
    'div',
    { class: 'pd-plate', 'aria-hidden': 'true' },
    h('i'),
    h('b', null, brand),
    text.product === brand ? null : h('span', null, text.product),
    h('i'),
  );
}

/** Where the face was scrolled, kept across redraws of the same face (seen only). */
export interface ScrollHold {
  face: string;
  top: number;
}

export interface EnclosureInput {
  ctx: Context;
  strip: Part;
  power: HTMLElement;
  lamps: Part;
  /** The open face, and its name (a tab, or the phase). */
  face: HTMLElement;
  faceName: string;
  /** The face is one of the four tabs' panels. */
  tabbed: boolean;
  /** The snapshot's error bands and the notes, above the face. */
  top: Child[];
  /** A refused intent's bands, at the face's foot. */
  refused: HTMLElement[];
  scroll: { held: ScrollHold | null; keep(hold: ScrollHold): void };
  /** Watching the face's size, replaced every frame. */
  watch(observer: ResizeObserver | null): void;
}

/** What drags the face: never a part, only bare metal and print. */
const PARTS = 'button, input, select, a, [role="slider"], [role="radiogroup"], .pd-map-frame, .pd-slide, .pd-rotary, .pd-dip';

/** The box for one frame: strip, face between the rails, foot, screws. */
export function enclosure(input: EnclosureInput): HTMLElement {
  const { ctx, face, faceName, tabbed } = input;
  const able = profileOpen(ctx.snapshot);
  const p = text.pedal;
  const scroller = h(
    'main',
    {
      class: 'pd-face',
      id: 'pd-face',
      role: tabbed ? 'tabpanel' : null,
      'aria-labelledby': tabbed ? `pd-tab-${ctx.view.tab}` : null,
      tabindex: 0,
      'data-face': faceName,
    },
    face,
  );
  const cue = (up: boolean) =>
    ikey({
      key: up ? 'face-up' : 'face-down',
      label: sr(up ? p.scrollUp : p.scrollDown),
      size: 'sm',
      off: true,
      onClick: () => turn(up ? -1 : 1),
    });
  const above = cue(true);
  const below = cue(false);
  above.classList.add('pd-cue', 'is-up');
  below.classList.add('pd-cue', 'is-down');
  const window_ = h('div', { class: 'pd-facewin' }, scroller, above, below);

  const reduce = typeof window.matchMedia === 'function' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
  function turn(sign: number) {
    const by = sign * scroller.clientHeight * 0.8;
    if (typeof scroller.scrollBy === 'function') scroller.scrollBy({ top: by, behavior: reduce ? 'auto' : 'smooth' });
    else scroller.scrollTop += by;
  }
  const light = (key: HTMLButtonElement, on: boolean) => {
    key.classList.toggle('is-lit', on);
    key.classList.toggle('is-off', !on);
    key.disabled = !on;
  };
  const update = () => {
    const more = scroller.scrollHeight > scroller.clientHeight + 1;
    const up = scroller.scrollTop > 1;
    const down = scroller.scrollTop + scroller.clientHeight < scroller.scrollHeight - 1;
    window_.classList.toggle('scrolls', more);
    window_.classList.toggle('more-up', more && up);
    window_.classList.toggle('more-down', more && down);
    light(above, more && up);
    light(below, more && down);
    input.scroll.keep({ face: faceName, top: scroller.scrollTop });
  };
  scroller.addEventListener('scroll', update, { passive: true });
  const keep = input.scroll.held !== null && input.scroll.held.face === faceName ? input.scroll.held.top : 0;
  const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(update) : null;
  input.watch(observer);
  queueMicrotask(() => {
    if (!scroller.isConnected) return;
    if (keep > 0) scroller.scrollTop = keep;
    update();
    observer?.observe(scroller);
    observer?.observe(face);
  });

  // Taking the plate by its bare metal: the face follows the hand one to one.
  let hold: { id: number; y: number; top: number; moved: boolean } | null = null;
  scroller.addEventListener('pointerdown', (event) => {
    if (event.pointerType === 'touch' || event.button !== 0) return;
    if (!window_.classList.contains('scrolls') || (event.target as Element | null)?.closest?.(PARTS)) return;
    hold = { id: event.pointerId, y: event.clientY, top: scroller.scrollTop, moved: false };
    event.preventDefault();
  });
  scroller.addEventListener('pointermove', (event) => {
    if (hold === null || event.pointerId !== hold.id) return;
    const dy = event.clientY - hold.y;
    if (!hold.moved && Math.abs(dy) < 3) return;
    if (!hold.moved) {
      hold.moved = true;
      scroller.setPointerCapture?.(hold.id);
      scroller.classList.add('is-dragging');
    }
    // The face is zoomed with the window; the pointer moves in window pixels.
    const zoom = parseFloat(getComputedStyle(scroller).getPropertyValue('--pd-k')) || 1;
    scroller.scrollTop = hold.top - dy / zoom;
  });
  const release = (event: PointerEvent) => {
    if (hold === null || event.pointerId !== hold.id) return;
    if (scroller.hasPointerCapture?.(hold.id)) scroller.releasePointerCapture?.(hold.id);
    scroller.classList.remove('is-dragging');
    hold = null;
  };
  scroller.addEventListener('pointerup', release);
  scroller.addEventListener('pointercancel', release);

  const screws = ['tl', 'tr', 'bl', 'br'].map((corner) => h('span', { class: `pd-screw is-${corner}`, 'aria-hidden': 'true' }));
  return h(
    'div',
    { class: 'pd-room', 'data-phase': ctx.snapshot.phase.kind },
    metalDefs(),
    h(
      'div',
      { class: 'pd-box' },
      input.strip.element,
      h(
        'div',
        { class: 'pd-faceframe' },
        input.top.some((child) => child !== null && child !== false && child !== undefined)
          ? h('div', { class: 'pd-top', 'aria-live': 'polite' }, ...input.top)
          : null,
        window_,
        input.refused.length === 0 ? null : h('div', { class: 'pd-refusals' }, ...input.refused),
      ),
      h(
        'footer',
        { class: 'pd-foot' },
        faceKeys(ctx, able),
        nameplate(),
        h('div', { class: 'pd-power' }, input.lamps.element, input.power),
      ),
      ...screws,
    ),
  );
}
