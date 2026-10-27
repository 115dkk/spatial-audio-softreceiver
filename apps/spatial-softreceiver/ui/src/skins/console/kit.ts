// The console skin's building blocks (ADR 0041, the console's law): the
// controls every page uses, each of which also files itself as a command for
// the palette (law 5), the form rows and sections, the status badges and the
// entrance marks (law 4: a colour always comes with a shape).

import type { Intent } from '../../bindings';
import { type Child, h, svg } from '../../dom';
import type { Context } from '../../screens/common';
import { text } from '../../text';

export type Tone = 'good' | 'warn' | 'crit' | 'off';

export interface Command {
  /** The control's data-key, or a go-to command's own id. */
  id: string;
  label: string;
  /** What the command belongs to (a section, an entrance), shown beside the label. */
  group: string | null;
  /** What running it does when the label alone does not say (a switch's 켜기 or 끄기). */
  action: string | null;
  disabled: boolean;
  /** Why it cannot run now. */
  reason: string | null;
  run(): void;
}

/** An in-page anchor the sidebar and the palette jump to. */
export interface Section {
  id: string;
  label: string;
}

/** One frame's drawing: the context, and what the drawing files as it goes. */
export interface Build {
  readonly ctx: Context;
  readonly commands: Command[];
  readonly sections: Section[];
  /** The group new commands are filed under. */
  group: string | null;
}

export function build(ctx: Context): Build {
  return { ctx, commands: [], sections: [], group: null };
}

export const cls = (...names: (string | false | null | undefined)[]): string => names.filter(Boolean).join(' ');

/** Why a control is locked: its own reason first, then an intent in flight. */
export function lockReason(b: Build, locked: string | null | undefined, followsBusy = true): string | null {
  if (locked !== null && locked !== undefined) return locked;
  return followsBusy && b.ctx.busy ? text.console.busy : null;
}

interface CommandInput {
  id: string;
  label: string;
  action?: string | null;
  group?: string | null;
  reason: string | null;
  run(): void;
}

export function command(b: Build, input: CommandInput): void {
  b.commands.push({
    id: input.id,
    label: input.label,
    group: input.group === undefined ? b.group : input.group,
    action: input.action ?? null,
    disabled: input.reason !== null,
    reason: input.reason,
    run: input.run,
  });
}

/** A busy button's content: the label kept for its width, the spinner over it, the spoken words. */
function spinnerLabel(label: string, spoken: string): Child[] {
  return [
    h('span', { class: 'cn-button-label', 'aria-hidden': 'true' }, label),
    h('span', { class: 'cn-spinner', 'aria-hidden': 'true' }),
    h('span', { class: 'cn-visually-hidden' }, spoken),
  ];
}

export type Variant = 'ink' | 'line' | 'quiet';

export interface ActOptions {
  key: string;
  variant?: Variant;
  pendingLabel?: string;
  /** Show the intent in flight as a spinner over the label (the width holds). */
  spinner?: boolean;
  /** Why the control cannot be used now, beyond an intent in flight. */
  locked?: string | null;
  describedBy?: string | null;
  /** False when the intent in flight is another control's (another endpoint's editor). */
  mine?: boolean;
  className?: string;
}

/** A button that sends one intent; it is also the palette's command of the same name. */
export function act(b: Build, label: string, intent: Intent, o: ActOptions): HTMLButtonElement {
  const { ctx } = b;
  ctx.offer(intent.kind);
  const pending = ctx.pending === intent.kind && o.mine !== false;
  const reason = lockReason(b, o.locked);
  const spinning = pending && o.spinner === true;
  const button = h(
    'button',
    {
      type: 'button',
      class: cls('cn-button', `is-${o.variant ?? 'line'}`, o.className, spinning && 'is-spinning'),
      'data-key': o.key,
      'data-intent': intent.kind,
      disabled: reason !== null,
      'aria-busy': pending ? 'true' : null,
      'aria-describedby': o.describedBy ?? null,
      onClick: () => ctx.send(intent),
    },
    ...(spinning
      ? spinnerLabel(label, o.pendingLabel ?? label)
      : [pending && o.pendingLabel !== undefined ? o.pendingLabel : label]),
  );
  command(b, { id: o.key, label, reason, run: () => ctx.send(intent) });
  return button;
}

export interface PlainOptions {
  key: string;
  variant?: Variant;
  locked?: string | null;
  /** Locked while an intent is in flight (true for anything that leads to one). */
  followsBusy?: boolean;
  describedBy?: string | null;
  /** File it as a palette command (default true). */
  command?: boolean;
  className?: string;
  attrs?: Record<string, string | null>;
}

/** A button for a view change or an OS call. */
export function plain(b: Build, label: string, run: () => void, o: PlainOptions): HTMLButtonElement {
  const reason = lockReason(b, o.locked, o.followsBusy ?? false);
  const button = h(
    'button',
    {
      type: 'button',
      class: cls('cn-button', `is-${o.variant ?? 'line'}`, o.className),
      'data-key': o.key,
      disabled: reason !== null,
      'aria-describedby': o.describedBy ?? null,
      onClick: run,
      ...(o.attrs ?? {}),
    },
    label,
  );
  if (o.command !== false) command(b, { id: o.key, label, reason, run });
  return button;
}

/** A button that asks the shell for a file or a folder and sends the path picked. */
export function pick(
  b: Build,
  label: string,
  key: string,
  locked: string | null,
  describedBy: string | null,
  choose: () => Promise<string | null>,
  send: (path: string) => void,
): HTMLButtonElement {
  const run = () => {
    choose().then(
      (path) => {
        if (path !== null) send(path);
      },
      (reason: unknown) => b.ctx.report({ message: String(reason), next: 'none' }),
    );
  };
  return plain(b, label, run, { key, locked, followsBusy: true, describedBy });
}

export interface ToggleOptions {
  key: string;
  label: string;
  /** On or off, as the switch shows it (a change in flight already stands where it goes). */
  on: boolean;
  locked?: string | null;
  followsBusy?: boolean;
  /** The change is in flight. */
  busy: boolean;
  describedBy: string | null;
  onToggle(on: boolean): void;
}

/** An on/off switch (`role="switch"`), its label inside so the whole row is the target. */
export function toggle(b: Build, o: ToggleOptions): HTMLButtonElement {
  const reason = lockReason(b, o.locked, o.followsBusy ?? true);
  const run = () => o.onToggle(!o.on);
  const element = h(
    'button',
    {
      type: 'button',
      role: 'switch',
      id: o.key,
      class: 'cn-switch',
      'data-key': o.key,
      'aria-checked': o.on ? 'true' : 'false',
      'aria-busy': o.busy ? 'true' : null,
      'aria-describedby': o.describedBy,
      disabled: reason !== null,
      onClick: run,
    },
    h('span', { class: 'cn-switch-track', 'aria-hidden': 'true' }, h('span', { class: 'cn-switch-thumb' })),
    h('span', { class: 'cn-switch-label' }, o.label),
  );
  command(b, {
    id: o.key,
    label: o.label,
    action: o.on ? text.status.powerOff : text.status.powerOn,
    reason,
    run,
  });
  return element;
}

export interface SelectOptions<T extends string> {
  id: string;
  key: string;
  label: string;
  options: readonly (readonly [T, string])[];
  /** The value shown; null shows `placeholder` (a value no option has). */
  current: T | null;
  placeholder?: string;
  /** Options shown but not choosable, with why. */
  unavailable?: ReadonlyMap<T, string>;
  locked?: string | null;
  busy?: boolean;
  describedBy?: string | null;
  labelledBy?: string;
  onChoose(value: T): void;
}

/** A select; each option is also a palette command "label: option". */
export function choose<T extends string>(b: Build, o: SelectOptions<T>): HTMLSelectElement {
  const reason = lockReason(b, o.locked);
  const select = h('select', {
    id: o.id,
    class: 'cn-select',
    'data-key': o.key,
    disabled: reason !== null,
    'aria-busy': o.busy === true ? 'true' : null,
    'aria-describedby': o.describedBy ?? null,
    'aria-labelledby': o.labelledBy ?? null,
  });
  if (o.current === null && o.placeholder !== undefined) {
    select.append(h('option', { value: '', selected: true }, o.placeholder));
  }
  for (const [value, label] of o.options) {
    select.append(h('option', { value, selected: value === o.current, disabled: o.unavailable?.has(value) === true }, label));
  }
  select.addEventListener('change', () => {
    const found = o.options.find(([value]) => value === select.value);
    if (found !== undefined) o.onChoose(found[0]);
  });
  for (const [value, label] of o.options) {
    const current = value === o.current;
    command(b, {
      id: `${o.key}=${value}`,
      label: `${o.label}: ${label}`,
      reason: reason ?? o.unavailable?.get(value) ?? (current ? text.console.current : null),
      run: () => o.onChoose(value),
    });
  }
  return select;
}

export interface RangeOptions {
  id: string;
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  /** The words for a value: the readout and the spoken value. */
  say(value: number): string;
  minLabel: string;
  maxLabel: string;
  locked?: string | null;
  describedBy?: string | null;
  onCommit(value: number): void;
}

/**
 * A slider with its ends and a readout that follows the thumb; the value is
 * sent once it is let go. Its palette command moves the focus to it.
 */
export function range(b: Build, o: RangeOptions): HTMLElement {
  const reason = lockReason(b, o.locked);
  const readout = h('output', { class: 'cn-readout cn-num', for: o.id }, o.say(o.value));
  const slider = h('input', {
    id: o.id,
    class: 'cn-range',
    type: 'range',
    min: o.min,
    max: o.max,
    step: o.step,
    value: o.value,
    'data-key': o.id,
    disabled: reason !== null,
    'aria-valuetext': o.say(o.value),
    'aria-describedby': o.describedBy ?? null,
  });
  slider.addEventListener('input', () => {
    const moved = o.say(Number(slider.value));
    readout.textContent = moved;
    slider.setAttribute('aria-valuetext', moved);
  });
  // `change` fires when the slider is let go (or a key step ends).
  slider.addEventListener('change', () => o.onCommit(Number(slider.value)));
  command(b, {
    id: o.id,
    label: o.label,
    action: text.console.adjust,
    reason,
    run: () => document.querySelector<HTMLElement>(`[data-key="${o.id}"]`)?.focus(),
  });
  return h(
    'div',
    { class: 'cn-range-row' },
    h('span', { class: 'cn-range-end', 'aria-hidden': 'true' }, o.minLabel),
    slider,
    h('span', { class: 'cn-range-end', 'aria-hidden': 'true' }, o.maxLabel),
    readout,
  );
}

/** A label | control row of a form. */
export function row(label: Child, controls: Child[], o: { for?: string; id?: string; className?: string } = {}): HTMLElement {
  const name =
    typeof label === 'string'
      ? o.for !== undefined
        ? h('label', { for: o.for, id: o.id ?? null }, label)
        : h('span', { id: o.id ?? null }, label)
      : label;
  return h(
    'div',
    { class: cls('cn-row', o.className) },
    h('div', { class: 'cn-row-label' }, name),
    h('div', { class: 'cn-row-control' }, ...controls),
  );
}

export const hint = (sentence: string, id: string | null = null, className?: string): HTMLParagraphElement =>
  h('p', { class: cls('cn-hint', className), id }, sentence);

export const para = (sentence: string, className?: string): HTMLParagraphElement =>
  h('p', { class: cls('cn-text', className) }, sentence);

/** A page section with a heading the sidebar and the palette can jump to. */
export function section(
  b: Build,
  id: string,
  title: string,
  children: Child[],
  o: { aside?: Child; className?: string; nav?: boolean; mark?: Element } = {},
): HTMLElement {
  const sid = `cn-sec-${id}`;
  if (o.nav !== false) b.sections.push({ id: sid, label: title });
  return h(
    'section',
    { class: cls('cn-section', o.className), id: sid, 'aria-labelledby': `${sid}-title` },
    h(
      'header',
      { class: 'cn-section-head' },
      h('h2', { id: `${sid}-title`, class: 'cn-section-title', tabindex: -1 }, o.mark ?? null, title),
      o.aside ?? null,
    ),
    ...children,
  );
}

/** The glyph that says a tone without its colour: a tick, a bang, a cross, a dash. */
export function toneGlyph(tone: Tone | 'busy'): Element {
  if (tone === 'busy') return h('span', { class: 'cn-spinner', 'aria-hidden': 'true' });
  const paths: Record<Tone, string> = {
    good: 'M3.5 8.2l3 3 6-6.4',
    warn: 'M8 3.5v5.5M8 12v.2',
    crit: 'M4.5 4.5l7 7M11.5 4.5l-7 7',
    off: 'M4.5 8h7',
  };
  return svg(
    'svg',
    { class: 'cn-glyph', viewBox: '0 0 16 16', width: 16, height: 16, 'aria-hidden': 'true', focusable: 'false' },
    svg('path', { d: paths[tone], fill: 'none', stroke: 'currentColor', 'stroke-width': 2, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' }),
  );
}

/** A status word with its glyph, tinted by tone. */
export function badge(tone: Tone, label: string, className?: string): HTMLElement {
  return h('span', { class: cls('cn-badge', `tone-${tone}`, className) }, toneGlyph(tone), h('span', null, label));
}

/** The entrances in mark order: main, music, PASS, and the ASIO input. */
export const LANE_COUNT = 4;
export const ASIO_LANE = 3;

/** Each entrance's shape, in a unit box (radius about 1): circle, triangle, square, diamond. */
export const SHAPES: readonly string[] = [
  'M1 0A1 1 0 1 1-1 0A1 1 0 1 1 1 0Z',
  'M0-1.15L1.1 0.8H-1.1Z',
  'M-0.88-0.88H0.88V0.88H-0.88Z',
  'M0-1.2L1.2 0L0 1.2L-1.2 0Z',
];

export function shapeOf(lane: number): string {
  return SHAPES[lane] ?? SHAPES[0]!;
}

/** The entrance's mark: its shape in its colour. */
export function laneMark(lane: number, className?: string): SVGSVGElement {
  return svg(
    'svg',
    {
      class: cls('cn-mark', `cn-lane-${lane}`, className),
      viewBox: '-1.35 -1.35 2.7 2.7',
      width: 12,
      height: 12,
      'aria-hidden': 'true',
      focusable: 'false',
    },
    svg('path', { d: shapeOf(lane), fill: 'currentColor' }),
  );
}

/** An entrance's name and purpose; lane 3 is the ASIO input. */
export function entrance(lane: number): { name: string; purpose: string } {
  if (lane === ASIO_LANE) return { name: text.asio, purpose: text.console.asioPurpose };
  return text.endpoints[lane] ?? { name: String(lane), purpose: '' };
}

/** A table in its own scrolling box, so a narrow window scrolls the table, not the page. */
export function table(o: {
  labelledBy?: string;
  label?: string;
  head: (string | null)[];
  rows: HTMLTableRowElement[];
  className?: string;
}): HTMLElement {
  return h(
    'div',
    { class: 'cn-table-wrap' },
    h(
      'table',
      { class: cls('cn-table', o.className), 'aria-labelledby': o.labelledBy ?? null, 'aria-label': o.label ?? null },
      h(
        'thead',
        null,
        h('tr', null, ...o.head.map((cell) => (cell === null ? h('td', null) : h('th', { scope: 'col' }, cell)))),
      ),
      h('tbody', null, ...o.rows),
    ),
  );
}

/** The last part of a path, for a set or a file named by its folder. */
export function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part.length > 0);
  return parts.at(-1) ?? path;
}
