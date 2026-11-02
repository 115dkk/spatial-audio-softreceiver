// The pedal's parts (ADR 0046, 이펙터의 법칙 1, 10, 12): each constructor
// builds one real part once per frame and hands back setters for its live
// values. A setter only flips classes and attributes or sets text; nothing
// here builds an element after the part is made, so the 30 Hz meters never
// grow the page. Every part that can be used is a real control bound to the
// contract; a part that cannot be used now is disabled and its light is off
// (`is-off`), and a part locked only while another intent is in flight keeps
// its light.

import type { Intent } from '../../bindings';
import { type Child, type Props, h, setAttr, svg } from '../../dom';
import type { Context } from '../../screens/common';

/** An LED's colour: a state, the white of a key, or an input's own. */
export type Tone = 'on' | 'wait' | 'fault' | 'white' | `lane-${number}`;

export const toneColour = (tone: Tone): string =>
  tone.startsWith('lane-') ? `var(--pd-${tone})` : `var(--pd-led-${tone})`;

/** Printed words: a short name, a quieter line, a sentence of the face. */
export const print = (words: Child, className = 'pd-label', id: string | null = null) =>
  h('span', { class: className, id }, words);

/** A sentence printed in the face's print area. */
export const fine = (sentence: Child, id: string | null = null, className = '') =>
  h('p', { class: `pd-fine ${className}`.trim(), id }, sentence);

/** Rows of printed facts: the name small, the value in the face's ink. */
export function facts(rows: [string, Child][], className = ''): HTMLDListElement {
  const list = h('dl', { class: `pd-facts ${className}`.trim() });
  for (const [label, value] of rows) list.append(h('div', { class: 'pd-fact' }, h('dt', null, label), h('dd', null, value)));
  return list;
}

/** A group of parts inside a printed frame, its name on a knocked-out tab. */
export function group(id: string, title: string | null, children: Child[], className = ''): HTMLElement {
  return h(
    'section',
    { class: `pd-group ${className}`.trim(), 'aria-labelledby': title === null ? null : `${id}-title`, 'data-group': id },
    title === null ? null : h('h2', { id: `${id}-title`, class: 'pd-tab' }, title),
    ...children,
  );
}

/** Words only a screen reader hears. */
export const sr = (words: Child, id: string | null = null) => h('span', { class: 'pd-sr', id }, words);

// ---------------------------------------------------------------- LEDs

export type Glow = 'on' | 'dim' | 'off';

export interface Led {
  element: HTMLSpanElement;
  set(glow: Glow, tone?: Tone): void;
}

/** An LED in its holder: a machined ring with a lens tinted with its colour, lit or dark. */
export function led(tone: Tone, glow: Glow = 'off', size: 'sm' | 'md' | 'lg' = 'md'): Led {
  let now = tone;
  const element = h('span', {
    class: `pd-led is-${size}`,
    style: `--c:${toneColour(tone)}`,
    'data-glow': glow,
    'aria-hidden': 'true',
  });
  return {
    element,
    set(next, to) {
      setAttr(element, 'data-glow', next);
      if (to !== undefined && to !== now) {
        now = to;
        element.style.setProperty('--c', toneColour(to));
      }
    },
  };
}

// ---------------------------------------------------------------- seven-segment readouts

/*
 * Seven-segment digits drawn as polygons and slanted like a real readout.
 * Every segment of every cell is drawn; a lit one has `is-on`, a dark one
 * stays as a faint mark behind the smoked window.
 */
const seg = {
  H: (y: number, x0: number, x1: number) => `${x0},${y} ${x0 + 1},${y - 1} ${x1 - 1},${y - 1} ${x1},${y} ${x1 - 1},${y + 1} ${x0 + 1},${y + 1}`,
  V: (x: number, y0: number, y1: number) => `${x},${y0} ${x + 1},${y0 + 1} ${x + 1},${y1 - 1} ${x},${y1} ${x - 1},${y1 - 1} ${x - 1},${y0 + 1}`,
};
const [SL, SR, ST, SM, SB, SG] = [1.5, 9.5, 1.2, 9, 16.8, 0.3];
const SEGMENTS: Record<string, string> = {
  a: seg.H(ST, SL + SG, SR - SG),
  b: seg.V(SR, ST + SG, SM - SG),
  c: seg.V(SR, SM + SG, SB - SG),
  d: seg.H(SB, SL + SG, SR - SG),
  e: seg.V(SL, SM + SG, SB - SG),
  f: seg.V(SL, ST + SG, SM - SG),
  g: seg.H(SM, SL + SG, SR - SG),
};
const ORDER = 'abcdefg';
const GLYPHS: Record<string, string> = {
  '0': 'abcdef',
  '1': 'bc',
  '2': 'abdeg',
  '3': 'abcdg',
  '4': 'bcfg',
  '5': 'acdfg',
  '6': 'acdefg',
  '7': 'abc',
  '8': 'abcdefg',
  '9': 'abcdfg',
  '-': 'g',
  '−': 'g',
  ' ': '',
};
const CELL = 12;

const shift = (points: string, x: number) =>
  points
    .split(' ')
    .map((point) => {
      const [a, b] = point.split(',');
      return `${(Number(a) + x).toFixed(2)},${b}`;
    })
    .join(' ');

/** Cells of a string: a digit or sign each, a point riding on the cell before it. */
export function segCells(value: string, cells: number): { glyph: string; point: boolean }[] {
  const list: { glyph: string; point: boolean }[] = [];
  for (const character of value) {
    if (character === '.') {
      const last = list.at(-1);
      if (last !== undefined) last.point = true;
      else list.push({ glyph: ' ', point: true });
    } else if (character in GLYPHS) {
      list.push({ glyph: character, point: false });
    }
    // A plus, a comma or a unit has no place on a seven-segment cell.
  }
  // Wider than the readout: every cell shows a dash, as a real one overflows.
  if (list.length > cells) return Array.from({ length: cells }, () => ({ glyph: '-', point: false }));
  while (list.length < cells) list.unshift({ glyph: ' ', point: false });
  return list;
}

export interface Seg {
  element: HTMLElement;
  set(value: string): void;
}

/** A segment readout of `cells` digits, `height` px tall; `set` lights the segments. */
export function segment(cells: number, height: number, initial = ''): Seg {
  const marks: { segments: SVGPolygonElement[]; point: SVGCircleElement }[] = [];
  const parts: SVGElement[] = [];
  for (let cell = 0; cell < cells; cell += 1) {
    const x = cell * CELL;
    const segments = [...ORDER].map((name) => svg('polygon', { points: shift(SEGMENTS[name]!, x) }));
    const point = svg('circle', { cx: x + 11, cy: 16.8, r: 1.05 });
    marks.push({ segments, point });
    parts.push(...segments, point);
  }
  const width = cells * CELL + 2.2;
  const element = h(
    'span',
    { class: 'pd-seg', 'aria-hidden': 'true' },
    svg(
      'svg',
      {
        class: 'pd-digits',
        viewBox: `0 0 ${width} 18`,
        height,
        width: Number(((width / 18) * height).toFixed(2)),
        focusable: 'false',
      },
      svg('g', { transform: 'translate(2.2 0) skewX(-7)' }, ...parts),
    ),
  );
  let shown: string | null = null;
  const set = (value: string) => {
    if (value === shown) return;
    shown = value;
    segCells(value, cells).forEach((cell, index) => {
      const mark = marks[index];
      if (mark === undefined) return;
      const lit = GLYPHS[cell.glyph] ?? '';
      mark.segments.forEach((polygon, at) => polygon.classList.toggle('is-on', lit.includes(ORDER[at]!)));
      mark.point.classList.toggle('is-on', cell.point);
    });
  };
  set(initial);
  return { element, set };
}

/** A readout with its printed unit and what a screen reader hears. */
export function readout(
  cells: number,
  height: number,
  unit: string,
  initial = '',
  className = '',
): { element: HTMLElement; set(value: string, spoken?: string): void } {
  const digits = segment(cells, height, initial);
  const said = sr(initial === '' ? '' : `${initial} ${unit}`.trim());
  const element = h('span', { class: `pd-readout ${className}`.trim() }, digits.element, unit === '' ? null : print(unit, 'pd-unit'), said);
  return {
    element,
    set(value, spoken) {
      digits.set(value);
      const words = spoken ?? `${value} ${unit}`.trim();
      if (said.textContent !== words) said.textContent = words;
    },
  };
}

// ---------------------------------------------------------------- the LED bar graph

/** Levels at or under the floor light nothing. */
export const FLOOR_DB = -60;

/** One LED bar-graph part: a black housing, its segments tinted lenses in the input's colour. */
export function ladder(tone: Tone, segments = 24): { element: HTMLElement; set(fraction: number): void } {
  const lenses = Array.from({ length: segments }, () => h('i'));
  const element = h('span', { class: 'pd-ladder', style: `--c:${toneColour(tone)}`, 'aria-hidden': 'true' }, ...lenses);
  let lit = -1;
  return {
    element,
    set(fraction) {
      const count = Math.round(Math.min(1, Math.max(0, fraction)) * segments);
      if (count === lit) return;
      lit = count;
      lenses.forEach((lens, index) => lens.classList.toggle('is-on', index < count));
    },
  };
}

/** A loudness on the bar graph: 0 at the floor or silence, 1 at 0 dBFS. */
export const strength = (dbfs: number) => (Number.isFinite(dbfs) ? Math.min(1, Math.max(0, (dbfs - FLOOR_DB) / -FLOOR_DB)) : 0);

// ---------------------------------------------------------------- keys

export interface KeyOptions {
  /** Stable key so focus survives a redraw. */
  key: string;
  label: Child;
  /** A quieter printed line under the name. */
  detail?: Child | undefined;
  /** The key's own light is on. */
  lit?: boolean | undefined;
  /** The chosen one of a row: its name printed heavier. */
  chosen?: boolean | undefined;
  /** The light's colour (white unless said). */
  tone?: Tone | undefined;
  size?: 'sm' | 'md' | undefined;
  /** The name printed beside the key instead of under it. */
  beside?: boolean | undefined;
  /** Cannot be used now: disabled, its light off. */
  off?: boolean | undefined;
  /** Another intent is in flight: disabled, its light kept. */
  locked?: boolean | undefined;
  /** This key's intent is in flight: its light blinks. */
  busy?: boolean | undefined;
  intent?: Intent['kind'] | undefined;
  describedBy?: string | null | undefined;
  /** More attributes (role, aria-checked, aria-selected and the like). */
  attrs?: Props | undefined;
  onClick(): void;
}

function keyText(options: KeyOptions): HTMLElement {
  return h(
    'span',
    { class: 'pd-key-text' },
    h('span', { class: 'pd-key-label' }, options.label),
    options.detail === undefined ? null : h('span', { class: 'pd-key-detail' }, options.detail),
  );
}

function keyProps(options: KeyOptions, className: string): Props {
  return {
    type: 'button',
    class: [
      'pd-key',
      className,
      options.lit && !options.off ? 'is-lit' : null,
      options.off ? 'is-off' : null,
      options.chosen ? 'is-chosen' : null,
      options.size === 'sm' ? 'is-sm' : null,
      options.beside ? 'is-beside' : null,
    ]
      .filter(Boolean)
      .join(' '),
    style: options.tone === undefined ? null : `--c:${toneColour(options.tone)}`,
    'data-key': options.key,
    'data-intent': options.intent ?? null,
    disabled: (options.off ?? false) || (options.locked ?? false),
    'aria-busy': options.busy ? 'true' : null,
    'aria-describedby': options.describedBy ?? null,
    ...options.attrs,
    onClick: options.onClick,
  };
}

/**
 * An illuminated push button: a machined bezel, a light ring, and in it a
 * dished metal actuator that sinks when pressed. The ring is the button's
 * own light; the state it stands for shows only there.
 */
export function ikey(options: KeyOptions): HTMLButtonElement {
  return h(
    'button',
    keyProps(options, 'pd-ikey-key'),
    h('span', { class: 'pd-ikey', 'aria-hidden': 'true' }, h('span', { class: 'pd-ikey-act' })),
    keyText(options),
  );
}

/**
 * A plain momentary key for an action that leaves no state to light: a
 * rectangular slot and a machined keycap that sinks when pressed, and stays
 * down while its intent is in flight.
 */
export function mkey(options: Omit<KeyOptions, 'lit' | 'tone'>): HTMLButtonElement {
  return h(
    'button',
    keyProps(options, 'pd-mkey-key'),
    h('span', { class: 'pd-mkey', 'aria-hidden': 'true' }, h('span', { class: 'pd-mkey-cap' })),
    keyText(options),
  );
}

/** A momentary key that sends one intent; locked while any intent is in flight. */
export function intentKey(
  ctx: Context,
  label: string,
  intent: Intent,
  options: { key: string; detail?: Child | undefined; off?: boolean | undefined; describedBy?: string | null | undefined; busyLabel?: string | undefined },
): HTMLButtonElement {
  ctx.offer(intent.kind);
  const busy = ctx.pending === intent.kind;
  return mkey({
    key: options.key,
    label: busy && options.busyLabel !== undefined ? options.busyLabel : label,
    detail: options.detail,
    off: options.off,
    locked: ctx.busy,
    busy,
    intent: intent.kind,
    describedBy: options.describedBy,
    onClick: () => ctx.send(intent),
  });
}

/** A momentary key that asks the shell for a file or a folder and sends the path picked. */
export function pickKey(
  ctx: Context,
  label: string,
  key: string,
  off: boolean,
  describedBy: string | null,
  pick: () => Promise<string | null>,
  send: (path: string) => void,
): HTMLButtonElement {
  return mkey({
    key,
    label,
    off,
    locked: ctx.busy,
    describedBy,
    onClick: () => {
      pick().then(
        (path) => {
          if (path !== null) send(path);
        },
        (reason: unknown) => ctx.report({ message: String(reason), next: 'none' }),
      );
    },
  });
}

/**
 * Arrow keys walk a group's enabled buttons and move the Tab stop with the
 * focus; Space or Enter (the button's own) chooses, so moving sends nothing.
 */
export function roving(group: HTMLElement, buttons: HTMLButtonElement[]): void {
  group.addEventListener('keydown', (event) => {
    const key = event.key;
    const enabled = buttons.filter((button) => !button.disabled);
    const at = enabled.indexOf(document.activeElement as HTMLButtonElement);
    if (at < 0 || enabled.length === 0) return;
    const to =
      key === 'ArrowDown' || key === 'ArrowRight' ? (at + 1) % enabled.length
      : key === 'ArrowUp' || key === 'ArrowLeft' ? (at - 1 + enabled.length) % enabled.length
      : key === 'Home' ? 0
      : key === 'End' ? enabled.length - 1
      : null;
    if (to === null) return;
    event.preventDefault();
    for (const button of buttons) button.tabIndex = -1;
    const target = enabled[to]!;
    target.tabIndex = 0;
    target.focus();
  });
}

export interface Choice<T> {
  value: T;
  label: string;
  key: string;
  detail?: string | undefined;
  /** This choice cannot be taken now (its key is off). */
  off?: boolean | undefined;
  describedBy?: string | null | undefined;
  tone?: Tone | undefined;
}

export interface KeyRowOptions<T> {
  label?: string | undefined;
  labelledBy?: string | undefined;
  describedBy?: string | null | undefined;
  choices: Choice<T>[];
  chosen: T | null;
  /** Every key off: the row cannot be used now. */
  off: boolean;
  /** Another intent is in flight. */
  locked: boolean;
  busy: boolean;
  /** The value whose intent is in flight, when it is not the chosen one: its key blinks. */
  sending?: T | null | undefined;
  intent?: Intent['kind'] | undefined;
  /** The keys in lines of these many each; the arrows still walk them all. */
  lines?: readonly number[] | undefined;
  size?: 'sm' | 'md' | undefined;
  choose(value: T): void;
}

/**
 * A row of illuminated keys with printed names, one radio group: the chosen
 * key's ring is lit. For the choices whose number is not fixed (outputs,
 * apps) and for the fixed ones of more than three (layouts).
 */
export function keyRow<T>(options: KeyRowOptions<T>): HTMLElement {
  const group = h('div', {
    role: 'radiogroup',
    class: `pd-keyrow${options.lines === undefined ? '' : ' is-lined'}`,
    'aria-label': options.label ?? null,
    'aria-labelledby': options.labelledBy ?? null,
    'aria-describedby': options.describedBy ?? null,
    'aria-busy': options.busy ? 'true' : null,
  });
  const chosenAt = options.choices.findIndex((choice) => choice.value === options.chosen);
  const buttons = options.choices.map((choice, index) => {
    const checked = index === chosenAt;
    return ikey({
      key: choice.key,
      label: choice.label,
      detail: choice.detail,
      lit: checked,
      chosen: checked,
      tone: choice.tone,
      size: options.size,
      off: options.off || (choice.off ?? false),
      locked: options.locked,
      busy: options.busy && (options.sending === undefined || options.sending === null ? checked : choice.value === options.sending),
      intent: options.intent,
      describedBy: choice.describedBy,
      attrs: {
        role: 'radio',
        'aria-checked': checked ? 'true' : 'false',
        tabindex: index === Math.max(chosenAt, 0) ? 0 : -1,
      },
      onClick: () => {
        if (!checked) options.choose(choice.value);
      },
    });
  });
  if (options.lines !== undefined) {
    let start = 0;
    for (const size of options.lines) {
      group.append(h('div', { class: 'pd-keyline' }, ...buttons.slice(start, start + size)));
      start += size;
    }
    if (start < buttons.length) group.append(h('div', { class: 'pd-keyline' }, ...buttons.slice(start)));
  } else {
    group.append(...buttons);
  }
  roving(group, buttons);
  return group;
}

// ---------------------------------------------------------------- slide switches

export interface SlideOptions<T> {
  /** The group's stable key (the gallery waits on it); the stops have their own. */
  key?: string | undefined;
  label?: string | undefined;
  labelledBy?: string | undefined;
  describedBy?: string | null | undefined;
  stops: Choice<T>[];
  at: T;
  off: boolean;
  locked: boolean;
  busy: boolean;
  intent?: Intent['kind'] | undefined;
  /**
   * A stop that cannot be taken but stays reachable, so its reason can be
   * read (`aria-disabled` and the reason's id): the headphones without a
   * BRIR set.
   */
  refuse?: ((value: T) => string | null) | undefined;
  choose(value: T): void;
}

/**
 * A slide switch of two or three stops: a slot cut through the plate and a
 * ribbed actuator that slides to the chosen stop. The printed names are the
 * radios; pressing the slot moves the actuator one stop on.
 */
export function slide<T>(options: SlideOptions<T>): HTMLElement {
  const count = options.stops.length;
  const at = Math.max(
    0,
    options.stops.findIndex((stop) => stop.value === options.at),
  );
  const locked = options.off || options.locked;
  const buttons = options.stops.map((stop, index) => {
    const checked = index === at;
    const refused = options.refuse?.(stop.value) ?? null;
    return h(
      'button',
      {
        type: 'button',
        role: 'radio',
        class: `pd-slide-stop${checked ? ' is-chosen' : ''}`,
        'data-key': stop.key,
        'data-intent': options.intent ?? null,
        'aria-checked': checked ? 'true' : 'false',
        'aria-disabled': refused !== null ? 'true' : null,
        'aria-describedby': refused ?? stop.describedBy ?? null,
        tabindex: checked ? 0 : -1,
        disabled: locked || (stop.off ?? false),
        style: count > 2 ? `--i:${index}` : null,
        onClick: () => {
          if (refused !== null || checked) return;
          options.choose(stop.value);
        },
      },
      stop.label,
    );
  });
  const hole = h(
    'span',
    {
      class: 'pd-slide-hole',
      'aria-hidden': 'true',
      onClick: () => {
        if (locked) return;
        for (let step = 1; step < count; step += 1) {
          const next = options.stops[(at + step) % count]!;
          const button = buttons[(at + step) % count]!;
          if (button.disabled || options.refuse?.(next.value)) continue;
          options.choose(next.value);
          return;
        }
      },
    },
    h('span', { class: 'pd-slide-knob' }),
  );
  const group = h(
    'div',
    {
      role: 'radiogroup',
      class: `pd-slide is-${count}${options.off ? ' is-off' : ''}`,
      style: `--n:${count};--at:${at}`,
      'data-key': options.key ?? null,
      'aria-label': options.label ?? null,
      'aria-labelledby': options.labelledBy ?? null,
      'aria-describedby': options.describedBy ?? null,
      'aria-busy': options.busy ? 'true' : null,
    },
    ...(count === 2 ? [buttons[0]!, hole, buttons[1]!] : [hole, h('span', { class: 'pd-slide-legend' }, ...buttons)]),
  );
  roving(group, buttons);
  return group;
}

// ---------------------------------------------------------------- knobs

/** A knob: black resin skirt, knurled, a machined cap with a milled pointer slot. */
function knobBody(size: number): { element: HTMLElement; turn(degrees: number): void } {
  const slots = ['shade', 'lip', 'cut'].map((kind) => h('span', { class: `pd-ptr is-${kind}` }));
  const element = h('span', { class: 'pd-knob', style: `--size:${size}px`, 'aria-hidden': 'true' }, h('span', { class: 'pd-knob-cap' }), ...slots);
  // The light stays at the top left however the knob turns: the slot's lip
  // and shade are the same line moved in screen space, then turned.
  const shifts: Record<string, string> = { shade: 'translate(-0.8px, -0.9px) ', lip: 'translate(1px, 1.1px) ', cut: '' };
  return {
    element,
    turn(degrees) {
      slots.forEach((slot, index) => {
        const kind = ['shade', 'lip', 'cut'][index]!;
        slot.style.transform = `${shifts[kind]}rotate(${(degrees + 180).toFixed(1)}deg)`;
      });
    },
  };
}

/** The encoder's turn, kept by the skin across frames (seen only, never saved). */
export interface KnobHold {
  drag: number | null;
  wheel: ReturnType<typeof setTimeout> | null;
}

export interface EncoderOptions {
  key: string;
  label: string;
  min: number;
  max: number;
  step: number;
  /** What the knob returns to: the value in flight or the snapshot's. */
  settled: number;
  /** What it shows now (its turn while turning). */
  value: number;
  /** LEDs on the ring (none for a plain pointer knob). */
  ring: number;
  size: number;
  off: boolean;
  locked: boolean;
  busy: boolean;
  intent?: Intent['kind'] | undefined;
  describedBy?: string | null | undefined;
  /** The value as words, for `aria-valuetext`. */
  say(value: number): string;
  hold: KnobHold;
  /** Every step of a turn, before anything is sent. */
  turned(value: number): void;
  commit(value: number): void;
}

/** The ring spans 270°, from lower left to lower right. */
const SWEEP = 270;

/**
 * A knob (`role="slider"`), with an LED ring when it is an encoder. Drag,
 * the wheel and the keys turn it; the value is sent when it is let go.
 */
export function encoder(options: EncoderOptions): HTMLElement {
  const { min, max, step, hold } = options;
  const blocked = options.off || options.locked;
  const span = max - min;
  const fraction = (value: number) => (value - min) / span;
  const snap = (value: number) => Math.min(max, Math.max(min, Math.round(value / step) * step));
  const body = knobBody(options.size);
  const outer = options.size / 2 + 12;
  const box = options.size + 40;
  const centre = box / 2;
  const leds = Array.from({ length: options.ring }, (_, index) => {
    const turn = ((-SWEEP / 2 + (SWEEP * index) / Math.max(1, options.ring - 1)) * Math.PI) / 180;
    return h('i', {
      class: 'pd-ring-led',
      style: `left:${(centre + outer * Math.sin(turn)).toFixed(1)}px;top:${(centre - outer * Math.cos(turn)).toFixed(1)}px`,
    });
  });
  const knob = h(
    'div',
    {
      role: 'slider',
      class: `pd-encoder${options.off ? ' is-off' : ''}`,
      style: `width:${box}px;height:${box}px`,
      tabindex: options.off ? -1 : 0,
      'data-key': options.key,
      'data-intent': options.intent ?? null,
      'aria-label': options.label,
      'aria-valuemin': min,
      'aria-valuemax': max,
      'aria-disabled': blocked ? 'true' : null,
      'aria-busy': options.busy ? 'true' : null,
      'aria-describedby': options.describedBy ?? null,
    },
    options.ring === 0 ? null : h('span', { class: 'pd-ring', 'aria-hidden': 'true' }, ...leds),
    body.element,
  );
  let value = options.value;
  const show = (next: number) => {
    value = next;
    const lit = fraction(next) * (options.ring - 1) + 1e-6;
    leds.forEach((dot, index) => dot.classList.toggle('is-on', !options.off && index <= lit));
    body.turn(-SWEEP / 2 + SWEEP * fraction(next));
    knob.setAttribute('aria-valuenow', String(next));
    knob.setAttribute('aria-valuetext', options.say(next));
  };
  show(value);

  const turn = (to: number) => {
    const next = snap(to);
    hold.drag = next;
    show(next);
    options.turned(next);
  };
  const commit = () => {
    if (hold.wheel !== null) clearTimeout(hold.wheel);
    hold.wheel = null;
    const wanted = hold.drag;
    hold.drag = null;
    if (wanted === null || blocked || wanted === options.settled) return;
    options.commit(wanted);
  };
  const keys: Record<string, number> = {
    ArrowUp: step,
    ArrowRight: step,
    ArrowDown: -step,
    ArrowLeft: -step,
    PageUp: step * 10,
    PageDown: -step * 10,
  };
  knob.addEventListener('keydown', (event) => {
    if (blocked) return;
    const by = keys[event.key];
    if (by !== undefined) turn(value + by);
    else if (event.key === 'Home') turn(min);
    else if (event.key === 'End') turn(max);
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
      if (blocked) return;
      event.preventDefault();
      turn(value + (event.deltaY < 0 ? step : -step));
      if (hold.wheel !== null) clearTimeout(hold.wheel);
      hold.wheel = setTimeout(commit, 450);
    },
    { passive: false },
  );
  let start: { x: number; y: number; value: number } | null = null;
  knob.addEventListener('pointerdown', (event) => {
    if (blocked) return;
    knob.setPointerCapture?.(event.pointerId);
    start = { x: event.clientX, y: event.clientY, value };
  });
  knob.addEventListener('pointermove', (event) => {
    if (start === null) return;
    // Up or right turns it up: one step every 4 px.
    const moved = start.y - event.clientY + (event.clientX - start.x);
    turn(start.value + (moved / 4) * step);
  });
  const release = (event: PointerEvent) => {
    if (start === null) return;
    start = null;
    knob.releasePointerCapture?.(event.pointerId);
    commit();
  };
  knob.addEventListener('pointerup', release);
  knob.addEventListener('pointercancel', release);
  return knob;
}

export interface RotaryOptions<T> {
  labelledBy?: string | undefined;
  label?: string | undefined;
  describedBy?: string | null | undefined;
  stops: Choice<T>[];
  at: T;
  off: boolean;
  locked: boolean;
  busy: boolean;
  intent?: Intent['kind'] | undefined;
  size?: number | undefined;
  choose(value: T): void;
}

/**
 * A rotary selector: a pointer knob with its positions printed round its
 * right side, a tick and a name at each. The names are the radios; pressing
 * the knob turns it one position on, as a detented selector does. The names
 * stand to the right so a long one wraps in the room the face gives it.
 */
export function rotary<T>(options: RotaryOptions<T>): HTMLElement {
  const size = options.size ?? 44;
  const count = options.stops.length;
  const spread = count === 2 ? 60 : count === 3 ? 90 : 120;
  // Clockwise from twelve o'clock: the positions fan out round three o'clock.
  const angles = options.stops.map((_, index) => 90 + (count === 1 ? 0 : -spread / 2 + (spread * index) / (count - 1)));
  const at = Math.max(
    0,
    options.stops.findIndex((stop) => stop.value === options.at),
  );
  const blocked = options.off || options.locked;
  const radians = (degrees: number) => (degrees * Math.PI) / 180;
  const reach = size / 2 + 16;
  const rise = Math.max(...angles.map((angle) => Math.abs(reach * Math.cos(radians(angle)))));
  const height = Math.ceil(Math.max(size + 20, 2 * (rise + 14)));
  const cx = size / 2 + 10;
  const cy = height / 2;
  const ticks = svg(
    'svg',
    { class: 'pd-ticks', width: cx + reach, height, viewBox: `0 0 ${cx + reach} ${height}`, 'aria-hidden': 'true', focusable: 'false' },
    ...angles.map((angle) => {
      const [r0, r1] = [size / 2 + 4, size / 2 + 10];
      const a = radians(angle);
      return svg('line', {
        x1: (cx + r0 * Math.sin(a)).toFixed(1),
        y1: (cy - r0 * Math.cos(a)).toFixed(1),
        x2: (cx + r1 * Math.sin(a)).toFixed(1),
        y2: (cy - r1 * Math.cos(a)).toFixed(1),
      });
    }),
  );
  const body = knobBody(size);
  body.turn(angles[at] ?? 90);
  const knob = h(
    'span',
    {
      class: 'pd-rotary-knob',
      style: `left:${cx - size / 2}px;top:${cy - size / 2}px`,
      'aria-hidden': 'true',
      onClick: () => {
        if (blocked) return;
        const next = options.stops[(at + 1) % count];
        if (next !== undefined && !(next.off ?? false) && count > 1) options.choose(next.value);
      },
    },
    body.element,
  );
  const buttons = options.stops.map((stop, index) => {
    const checked = index === at;
    const a = radians(angles[index] ?? 90);
    const x = cx + reach * Math.sin(a) + 2;
    const y = cy - reach * Math.cos(a);
    return h(
      'button',
      {
        type: 'button',
        role: 'radio',
        class: `pd-rotary-stop${checked ? ' is-chosen' : ''}`,
        style: `left:${x.toFixed(1)}px;top:${y.toFixed(1)}px;max-width:calc(100% - ${Math.ceil(x)}px)`,
        'data-key': stop.key,
        'data-intent': options.intent ?? null,
        'aria-checked': checked ? 'true' : 'false',
        'aria-describedby': stop.describedBy ?? null,
        tabindex: checked ? 0 : -1,
        disabled: blocked || (stop.off ?? false),
        onClick: () => {
          if (!checked) options.choose(stop.value);
        },
      },
      stop.label,
    );
  });
  const group = h(
    'div',
    {
      role: 'radiogroup',
      class: `pd-rotary${options.off ? ' is-off' : ''}`,
      style: `height:${height}px;min-width:${Math.ceil(cx + reach + 64)}px`,
      'aria-label': options.label ?? null,
      'aria-labelledby': options.labelledBy ?? null,
      'aria-describedby': options.describedBy ?? null,
      'aria-busy': options.busy ? 'true' : null,
    },
    ticks,
    knob,
    ...buttons,
  );
  roving(group, buttons);
  return group;
}

// ---------------------------------------------------------------- DIP switches

export interface DipRow {
  key: string;
  label: string;
  on: boolean;
  off: boolean;
  locked: boolean;
  busy: boolean;
  describedBy: string | null;
  toggle(on: boolean): void;
}

/**
 * A bank of DIP switches, the kind under a pedal's lid: a black body with a
 * white lever per position, the positions numbered and named beside it.
 * Each position is a toggle (`aria-pressed`) with its name inside.
 */
export function dip(rows: DipRow[], onWord: string): HTMLElement {
  return h(
    'div',
    { class: 'pd-dip' },
    h('span', { class: 'pd-dip-on', 'aria-hidden': 'true' }, onWord),
    ...rows.map((row, index) =>
      h(
        'button',
        {
          type: 'button',
          class: `pd-dip-row${row.on ? ' is-on' : ''}${row.off ? ' is-off' : ''}`,
          'data-key': row.key,
          'aria-pressed': row.on ? 'true' : 'false',
          'aria-busy': row.busy ? 'true' : null,
          'aria-describedby': row.describedBy,
          disabled: row.off || row.locked,
          onClick: () => row.toggle(!row.on),
        },
        h('span', { class: 'pd-dip-sw', 'aria-hidden': 'true' }, h('i')),
        h('b', { class: 'pd-dip-n', 'aria-hidden': 'true' }, String(index + 1)),
        h('span', { class: 'pd-dip-name' }, row.label),
      ),
    ),
  );
}

// ---------------------------------------------------------------- drawn hardware

const hexPoints = (cx: number, cy: number, r: number) =>
  Array.from({ length: 6 }, (_, index) => {
    const a = (Math.PI / 3) * index + Math.PI / 6;
    return `${(cx + r * Math.cos(a)).toFixed(2)},${(cy + r * Math.sin(a)).toFixed(2)}`;
  }).join(' ');

/** The gradients the drawn metal parts share; one copy per page. */
export function metalDefs(): SVGSVGElement {
  const stops = (...list: [number, string][]) => list.map(([offset, colour]) => svg('stop', { offset, style: `stop-color:${colour}` }));
  return svg(
    'svg',
    { class: 'pd-defs', width: 0, height: 0, 'aria-hidden': 'true', focusable: 'false' },
    svg(
      'defs',
      null,
      svg('linearGradient', { id: 'pd-g-nut', x1: 0, y1: 0, x2: 1, y2: 1 }, ...stops([0, 'var(--pd-nut-1)'], [0.55, 'var(--pd-nut-2)'], [1, 'var(--pd-nut-3)'])),
      svg('radialGradient', { id: 'pd-g-washer', cx: 0.38, cy: 0.32, r: 0.8 }, ...stops([0, 'var(--pd-nut-1)'], [0.6, 'var(--pd-nut-2)'], [1, 'var(--pd-nut-3)'])),
      svg('radialGradient', { id: 'pd-g-dome', cx: 0.4, cy: 0.34, r: 0.75 }, ...stops([0, '#ffffff'], [0.35, '#d8dadc'], [0.7, '#8e9194'], [1, '#cfd1d3'])),
    ),
  );
}

/** A jack on the top of the box, seen from above: hex nut, washer, knurled ring, the hole. */
export function jackDrawing(size = 40): SVGSVGElement {
  return svg(
    'svg',
    { class: 'pd-jack-nut', width: size, height: size, viewBox: '0 0 40 40', 'aria-hidden': 'true', focusable: 'false' },
    svg('polygon', { points: hexPoints(20, 20, 18.5), fill: 'url(#pd-g-nut)', stroke: 'rgba(0,0,0,.35)', 'stroke-width': 0.8 }),
    svg('circle', { cx: 20, cy: 20, r: 13.5, fill: 'url(#pd-g-washer)', stroke: 'rgba(0,0,0,.3)', 'stroke-width': 0.6 }),
    svg('circle', { cx: 20, cy: 20, r: 10, fill: 'none', stroke: 'rgba(0,0,0,.28)', 'stroke-width': 2.2, 'stroke-dasharray': '1 1.2' }),
    svg('circle', { cx: 20, cy: 20, r: 7, fill: '#0b0b0c' }),
    svg('circle', { cx: 20, cy: 20, r: 7, fill: 'none', stroke: 'rgba(255,255,255,.25)', 'stroke-width': 0.7 }),
    svg('circle', { cx: 19, cy: 21, r: 3.2, fill: '#202123' }),
  );
}

/** The footswitch: hex nut, washer and the domed plunger. */
export function footswitchDrawing(size = 72): SVGSVGElement {
  return svg(
    'svg',
    { class: 'pd-foot-drawing', width: size, height: size, viewBox: '0 0 72 72', 'aria-hidden': 'true', focusable: 'false' },
    svg('polygon', { points: hexPoints(36, 36, 34), fill: 'url(#pd-g-nut)', stroke: 'rgba(0,0,0,.35)', 'stroke-width': 1 }),
    svg('circle', { cx: 36, cy: 36, r: 26, fill: 'url(#pd-g-washer)', stroke: 'rgba(0,0,0,.35)', 'stroke-width': 0.8 }),
    svg('circle', { class: 'pd-plunger', cx: 36, cy: 36, r: 20, fill: 'url(#pd-g-dome)', stroke: 'rgba(0,0,0,.4)', 'stroke-width': 0.8 }),
    svg('circle', { cx: 36, cy: 36, r: 13, fill: 'none', stroke: 'rgba(255,255,255,.55)', 'stroke-width': 1 }),
    svg('ellipse', { cx: 30, cy: 28, rx: 7, ry: 4, fill: 'rgba(255,255,255,.75)', transform: 'rotate(-30 30 28)' }),
  );
}

/** The arrow printed between the columns of the signal's way. */
export function flowArrow(): SVGSVGElement {
  return svg(
    'svg',
    { class: 'pd-flow', width: 22, height: 14, viewBox: '0 0 22 14', 'aria-hidden': 'true', focusable: 'false' },
    svg('path', { d: 'M1 7h18M13 1.5 19.5 7 13 12.5', fill: 'none', stroke: 'currentColor', 'stroke-width': 1.6 }),
  );
}

/**
 * Laser etching (법칙 9): lines cut into the metal, a shade darker with a
 * bright lip on aluminium and a shade lighter on dark metal. No words.
 */
function etched(className: string, viewBox: string, art: SVGElement[], copy: () => SVGElement[]): SVGSVGElement {
  return svg(
    'svg',
    { class: className, viewBox, 'aria-hidden': 'true', focusable: 'false' },
    svg('g', { class: 'pd-etch-lip', transform: 'translate(0.7 0.9)' }, ...copy()),
    svg('g', { class: 'pd-etch-cut' }, ...art),
  );
}

/** The sound field etched round the map: rings spreading from just outside the printed plan. */
export function etchedField(): SVGSVGElement {
  const rings = () => {
    const list: SVGElement[] = [];
    for (let r = 152; r <= 780; r += 16) list.push(svg('circle', { cx: 800, cy: 800, r }));
    return list;
  };
  return etched('pd-etch-field', '0 0 1600 1600', rings(), rings);
}

/**
 * The machine's signal flow etched as line art: four input jacks gathering
 * into the decoder, two channels fanning out into many (upmix), the
 * listening place with its rings (spatial rendering), one output jack.
 */
export function etchedFlow(): SVGSVGElement {
  const art = () => {
    const parts: SVGElement[] = [];
    for (const y of [30, 62, 94, 126]) {
      parts.push(svg('circle', { cx: 40, cy: y, r: 11 }), svg('circle', { cx: 40, cy: y, r: 4 }));
      parts.push(svg('path', { d: `M51 ${y} C 110 ${y}, 120 78, 170 78` }));
    }
    parts.push(svg('rect', { x: 170, y: 48, width: 120, height: 60, rx: 8 }));
    [62, 72, 82, 92].forEach((y, index) => parts.push(svg('path', { d: `M186 ${y} H ${236 + index * 10}` })));
    parts.push(svg('path', { d: 'M290 70 H 340' }), svg('path', { d: 'M290 86 H 340' }));
    for (let index = 0; index < 8; index += 1) {
      const y = 22 + index * 16;
      const from = index < 4 ? 70 : 86;
      parts.push(svg('path', { d: `M340 ${from} C 400 ${from}, 400 ${y}, 460 ${y} H 520` }));
    }
    parts.push(svg('path', { d: 'M520 22 C 560 22, 560 78, 600 78' }), svg('path', { d: 'M520 134 C 560 134, 560 78, 600 78' }));
    for (const r of [22, 36, 50, 64]) parts.push(svg('circle', { cx: 680, cy: 78, r }));
    parts.push(svg('circle', { cx: 680, cy: 78, r: 10 }), svg('path', { d: 'M675 69 L680 61 L685 69' }));
    parts.push(svg('path', { d: 'M744 78 H 880' }), svg('circle', { cx: 902, cy: 78, r: 15 }), svg('circle', { cx: 902, cy: 78, r: 5.5 }));
    return parts;
  };
  return etched('pd-etch-flow', '0 0 940 156', art(), art);
}
