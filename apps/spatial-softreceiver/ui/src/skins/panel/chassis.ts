// The chassis (ADR 0041, 전면 패널의 법칙 1): only the parts a real AV
// receiver has on its front. The power button with its lamp ring, a signal
// lamp per input, the volume encoder with its LED ring, the output kind keys
// and the page keys under the display. Each part is a real control bound to
// the contract; the only words on the chassis are the short names printed by
// the parts. A part that cannot act in this phase is off: unlit and disabled.

import type { EngineStatus, Intent, ProfileKind, Snapshot } from '../../bindings';
import { h, setAttr, svg } from '../../dom';
import { oneDecimal } from '../../format';
import { text } from '../../text';
import type { Context, Tab } from '../../screens/common';
import { GAIN_MAX, GAIN_MIN } from '../../screens/profile';
import { LANES, type Scope, signed } from './scope';

const TABS: readonly Tab[] = ['status', 'profile', 'upmix', 'settings'];
const KINDS: readonly ProfileKind[] = ['headphones', 'speakers'];

/** The profile, the gain and the pages can change once the wizard is done. */
export function profileOpen(snapshot: Snapshot): boolean {
  const phase = snapshot.phase.kind;
  return phase === 'ready' || phase === 'running' || phase === 'faulted';
}

export type Lamp = 'off' | 'on' | 'wait' | 'fault';

/** The power lamp: green running, amber turning on or reconnecting, red stopped. */
export function powerLamp(snapshot: Snapshot, pending: Intent['kind'] | null, engine: EngineStatus | null): Lamp {
  if (pending === 'enable') return 'wait';
  switch (snapshot.phase.kind) {
    case 'running':
      return (engine ?? snapshot.engine)?.fault ? 'wait' : 'on';
    case 'faulted':
      return 'fault';
    default:
      return 'off';
  }
}

/** The power state as the status line says it. */
export function powerWord(lamp: Lamp, snapshot: Snapshot): string {
  switch (lamp) {
    case 'wait':
      return snapshot.phase.kind === 'running' ? text.status.reconnecting : text.status.powerTurningOn;
    case 'on':
      return text.status.on;
    case 'fault':
      return text.status.stopped;
    case 'off':
      return text.status.off;
  }
}

/** A short name printed on the chassis by its part. */
const print = (words: string, id: string | null = null) => h('span', { id, class: 'pn-print' }, words);

/** The power glyph: a broken ring and its stem. */
function powerGlyph(): SVGSVGElement {
  return svg(
    'svg',
    { class: 'pn-power-glyph', viewBox: '0 0 24 24', width: 20, height: 20, 'aria-hidden': 'true', focusable: 'false' },
    svg('path', { d: 'M7.05 6.6a7 7 0 1 0 9.9 0', fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round' }),
    svg('path', { d: 'M12 3.5v8', fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round' }),
  );
}

export interface Part {
  element: HTMLElement;
  update(engine: EngineStatus | null): void;
}

/**
 * The power button: enable while off or stopped, disable while running
 * (`data-key="power"`). Its name is the printed 전원 and what a press does;
 * the status line's word describes it.
 */
export function powerButton(ctx: Context): Part {
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
  const ring = h('span', { class: 'pn-power-ring', 'aria-hidden': 'true', 'data-lamp': powerLamp(snapshot, ctx.pending, null) });
  const button = h(
    'button',
    {
      type: 'button',
      id: 'pn-power',
      class: 'pn-power',
      'data-key': 'power',
      'data-intent': able ? intent.kind : null,
      disabled: !able || ctx.busy,
      'aria-busy': pending ? 'true' : null,
      'aria-labelledby': 'pn-power-print pn-power-action',
      'aria-describedby': 'pn-state-word',
      onClick: () => ctx.send(intent),
    },
    ring,
    powerGlyph(),
    h('span', { id: 'pn-power-action', class: 'pn-sr' }, action),
  );
  return {
    element: h('div', { class: 'pn-part pn-power-part' }, button, print(text.panel.power, 'pn-power-print')),
    update(engine) {
      setAttr(ring, 'data-lamp', powerLamp(snapshot, ctx.pending, engine));
    },
  };
}

type Signal = 'sound' | 'idle' | 'none';

/**
 * Each input's state for its lamp: sound now (an object of its lane in the
 * last mixed block), a format but silent, or nothing. The ASIO input is lit
 * while a DAW plays into it and dim while its sound waits on a stand-in clock.
 * With the app-sound entrance (ADR 0043) the lamps are its lanes: a lane
 * without an app stays dark, and so does PASS, which only the driver feeds.
 */
export function signals(engine: EngineStatus | null, lit: readonly number[] = [0, 1, 2]): Signal[] {
  const states: Signal[] = ['none', 'none', 'none', 'none'];
  if (engine === null) return states;
  for (let lane = 0; lane < LANES; lane += 1) {
    if (lit.includes(lane) && engine.endpoints[lane]?.format) states[lane] = 'idle';
  }
  for (const object of engine.scene) {
    if (object.lane < LANES && lit.includes(object.lane)) states[object.lane] = 'sound';
  }
  const asio = engine.asio ?? null;
  if (asio !== null && asio.fault_reason === null) states[LANES] = asio.clock_fault ? 'idle' : 'sound';
  return states;
}

/**
 * The input lamps: game, music, PASS and ASIO, each in its own colour.
 * Output only. `lit` are the lanes that can light (all three with the
 * driver entrance).
 */
export function signalLamps(lit: readonly number[] = [0, 1, 2]): Part {
  const p = text.panel;
  const words: Record<Signal, string> = { sound: p.lampSound, idle: p.lampIdle, none: text.status.noInput };
  const lamps = p.lamps.map((name, lane) => {
    const said = h('span', { class: 'pn-sr' }, words.none);
    const element = h(
      'li',
      { class: 'pn-lamp', 'data-lane': lane, 'data-signal': 'none' },
      h('span', { class: 'pn-led', 'aria-hidden': 'true' }),
      print(name),
      said,
    );
    return { element, said };
  });
  const element = h(
    'div',
    { class: 'pn-part pn-lamps-part' },
    h(
      'ul',
      { class: 'pn-lamps', 'aria-label': p.lampsLabel, 'aria-describedby': 'pn-lamps-hint' },
      ...lamps.map((lamp) => lamp.element),
    ),
    h('span', { id: 'pn-lamps-hint', class: 'pn-sr' }, p.lampsHint),
  );
  return {
    element,
    update(engine) {
      signals(engine, lit).forEach((signal, lane) => {
        const lamp = lamps[lane];
        if (lamp === undefined) return;
        setAttr(lamp.element, 'data-signal', signal);
        if (lamp.said.textContent !== words[signal]) lamp.said.textContent = words[signal];
      });
    },
  };
}

/** The encoder's turn, kept by the skin across frames (seen only, never saved). */
export interface EncoderHold {
  /** The gain while the encoder is being turned; null otherwise. */
  drag: number | null;
  wheel: ReturnType<typeof setTimeout> | null;
}

const STEP = 0.5;
const LEDS = 31;
/** The ring spans 270°, from lower left to lower right. */
const SWEEP = 270;

const fraction = (db: number) => (db - GAIN_MIN) / (GAIN_MAX - GAIN_MIN);
const snap = (db: number) => Math.min(GAIN_MAX, Math.max(GAIN_MIN, Math.round(db / STEP) * STEP));
const angle = (db: number) => -SWEEP / 2 + SWEEP * fraction(db);

/**
 * The volume encoder with its LED ring: the output gain (`set_gain`, the
 * standard slider's range and step). Drag, the wheel and the arrow keys turn
 * it; the gain is sent when it is let go, and the readouts on the display
 * follow it while it turns.
 */
export function encoder(scope: Scope, hold: EncoderHold, preview: (db: number) => void): HTMLElement {
  const { ctx } = scope;
  const able = profileOpen(ctx.snapshot);
  if (able) ctx.offer('set_gain');
  const locked = !able || ctx.busy;
  const settled = scope.gain.settled;
  let value = scope.gain.value();

  const leds = Array.from({ length: LEDS }, (_, index) => {
    const turn = ((-SWEEP / 2 + (SWEEP * index) / (LEDS - 1) - 90) * Math.PI) / 180;
    const zero = Math.abs(index / (LEDS - 1) - fraction(0)) < 0.5 / (LEDS - 1);
    return svg('circle', {
      class: `pn-ring-led${zero ? ' is-zero' : ''}`,
      cx: (50 + 45 * Math.cos(turn)).toFixed(2),
      cy: (50 + 45 * Math.sin(turn)).toFixed(2),
      r: zero ? 2.1 : 1.6,
    });
  });
  const ring = svg('svg', { class: 'pn-ring', viewBox: '0 0 100 100', 'aria-hidden': 'true', focusable: 'false' }, ...leds);
  // The spun cap keeps its highlights where the light is; only the pointer turns.
  const pointer = h('span', { class: 'pn-knob-turn', 'aria-hidden': 'true' }, h('span', { class: 'pn-knob-pointer' }));
  const knob = h(
    'div',
    {
      role: 'slider',
      class: 'pn-knob',
      id: 'pn-gain',
      tabindex: able ? 0 : -1,
      'data-key': 'gain',
      'data-intent': able ? 'set_gain' : null,
      'aria-label': text.profile.gainLabel,
      'aria-valuemin': GAIN_MIN,
      'aria-valuemax': GAIN_MAX,
      'aria-disabled': locked ? 'true' : null,
      'aria-busy': ctx.pending === 'set_gain' ? 'true' : null,
    },
    h('span', { class: 'pn-knob-face', 'aria-hidden': 'true' }),
    pointer,
  );

  const show = (db: number) => {
    value = db;
    const lit = fraction(db) * (LEDS - 1) + 1e-6;
    leds.forEach((led, index) => led.classList.toggle('is-lit', able && index <= lit));
    pointer.style.transform = `rotate(${angle(db).toFixed(1)}deg)`;
    knob.setAttribute('aria-valuenow', String(db));
    knob.setAttribute('aria-valuetext', `${oneDecimal(db)} dB`);
  };
  show(value);

  const turn = (db: number) => {
    const next = snap(db);
    hold.drag = next;
    show(next);
    preview(next);
    scope.osd(`${text.status.volume} ${signed(next)} dB`);
  };
  const commit = () => {
    if (hold.wheel !== null) clearTimeout(hold.wheel);
    hold.wheel = null;
    const wanted = hold.drag;
    hold.drag = null;
    if (wanted === null || ctx.busy) return;
    if (wanted === settled) return;
    ctx.setView({ gainSending: wanted });
    ctx.send({ kind: 'set_gain', value: wanted });
  };

  const keys: Record<string, number> = {
    ArrowUp: STEP,
    ArrowRight: STEP,
    ArrowDown: -STEP,
    ArrowLeft: -STEP,
    PageUp: STEP * 10,
    PageDown: -STEP * 10,
  };
  knob.addEventListener('keydown', (event) => {
    if (locked) return;
    const by = keys[event.key];
    if (by !== undefined) turn(value + by);
    else if (event.key === 'Home') turn(GAIN_MIN);
    else if (event.key === 'End') turn(GAIN_MAX);
    else return;
    event.preventDefault();
  });
  knob.addEventListener('keyup', (event) => {
    if (event.key in keys || event.key === 'Home' || event.key === 'End') commit();
  });
  knob.addEventListener('blur', () => {
    if (hold.drag !== null && knob.isConnected) commit();
  });
  knob.addEventListener(
    'wheel',
    (event) => {
      if (locked) return;
      event.preventDefault();
      turn(value + (event.deltaY < 0 ? STEP : -STEP));
      if (hold.wheel !== null) clearTimeout(hold.wheel);
      hold.wheel = setTimeout(commit, 450);
    },
    { passive: false },
  );
  let start: { x: number; y: number; db: number } | null = null;
  knob.addEventListener('pointerdown', (event) => {
    if (locked) return;
    knob.setPointerCapture?.(event.pointerId);
    start = { x: event.clientX, y: event.clientY, db: value };
  });
  knob.addEventListener('pointermove', (event) => {
    if (start === null) return;
    // Up or right turns it up: 0.5 dB every 4 px.
    const moved = start.y - event.clientY + (event.clientX - start.x);
    turn(start.db + (moved / 4) * STEP);
  });
  const release = (event: PointerEvent) => {
    if (start === null) return;
    start = null;
    knob.releasePointerCapture?.(event.pointerId);
    commit();
  };
  knob.addEventListener('pointerup', release);
  knob.addEventListener('pointercancel', release);

  return h(
    'div',
    { class: `pn-part pn-encoder-part${able ? '' : ' is-off'}` },
    h('div', { class: 'pn-encoder' }, h('span', { class: 'pn-ring-well', 'aria-hidden': 'true' }), ring, knob),
    print(text.status.volume),
  );
}

/**
 * The output kind keys: headphones and speakers (`set_profile_kind`), the LED
 * on the kind in use. A key that cannot be taken stays pressable and says why
 * on the display; arrow keys move between the two.
 */
export function outputKeys(scope: Scope): HTMLElement {
  const { ctx } = scope;
  const { profile } = ctx.snapshot;
  const able = profileOpen(ctx.snapshot);
  if (able) ctx.offer('set_profile_kind');
  const sending = ctx.sending?.kind === 'set_profile_kind' ? ctx.sending.value : null;
  const shown = sending ?? profile.kind;
  const needsBrir = profile.headphones.brir === null && !profile.from_file;
  const group = h('div', { class: 'pn-okeys', role: 'radiogroup', 'aria-label': text.profile.outputTitle });
  const buttons = KINDS.map((kind) => {
    const why = !able ? null : profile.from_file ? text.profile.fromFile : kind === 'headphones' && needsBrir ? text.profile.needsBrir : null;
    const whyId = `pn-kind-${kind}-why`;
    const chosen = able && kind === shown;
    const button = h(
      'button',
      {
        type: 'button',
        role: 'radio',
        class: 'pn-okey',
        'data-key': `kind-${kind}`,
        'data-intent': able ? 'set_profile_kind' : null,
        'aria-checked': chosen ? 'true' : 'false',
        'aria-disabled': why !== null ? 'true' : null,
        'aria-describedby': why !== null ? whyId : null,
        'aria-busy': sending === kind ? 'true' : null,
        tabindex: kind === shown ? 0 : -1,
        disabled: !able || ctx.busy,
        onClick: () => {
          if (why !== null) scope.osd(why);
          else if (kind !== profile.kind) {
            scope.osd(text.profile.kindNames[kind]);
            ctx.send({ kind: 'set_profile_kind', value: kind });
          }
        },
      },
      h('span', { class: 'pn-key-led', 'aria-hidden': 'true' }),
      h('span', { class: 'pn-key-print' }, text.profile.kindNames[kind]),
      why === null ? null : h('span', { id: whyId, class: 'pn-sr' }, why),
    );
    return button;
  });
  group.append(...buttons);
  group.addEventListener('keydown', (event) => {
    const at = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (at < 0) return;
    const to = event.key === 'ArrowRight' || event.key === 'ArrowDown' ? 1 : event.key === 'ArrowLeft' || event.key === 'ArrowUp' ? -1 : 0;
    if (to === 0) return;
    event.preventDefault();
    const target = buttons[(at + to + buttons.length) % buttons.length]!;
    for (const button of buttons) button.tabIndex = -1;
    target.tabIndex = 0;
    target.focus();
  });
  return h('div', { class: 'pn-part pn-okeys-part' }, group);
}

/**
 * The page keys under the display: a tablist whose panel is the display.
 * Choosing one only changes view state; in the wizard and after quitting
 * they are off.
 */
export function pageKeys(ctx: Context, able: boolean): HTMLElement {
  const choose = (tab: Tab, focus: boolean) => {
    if (focus) list.querySelector<HTMLElement>(`[data-key="nav-${tab}"]`)?.focus();
    ctx.setView({ tab });
  };
  const list = h('div', {
    class: 'pn-pagekeys',
    role: 'tablist',
    'aria-label': text.nav.label,
    onKeydown: (event: Event) => {
      if (!able) return;
      const key = (event as KeyboardEvent).key;
      // From the key that has the focus (a pointer may have put it on another).
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
      h(
        'button',
        {
          type: 'button',
          role: 'tab',
          id: `pn-tab-${tab}`,
          class: 'pn-pagekey',
          'data-key': `nav-${tab}`,
          'aria-selected': selected ? 'true' : 'false',
          'aria-controls': selected ? 'pn-page' : null,
          tabindex: selected ? 0 : -1,
          disabled: !able,
          onClick: () => choose(tab, false),
        },
        h('span', { class: 'pn-key-bar', 'aria-hidden': 'true' }),
        h('span', { class: 'pn-key-print' }, text.nav[tab]),
      ),
    );
  }
  return list;
}
