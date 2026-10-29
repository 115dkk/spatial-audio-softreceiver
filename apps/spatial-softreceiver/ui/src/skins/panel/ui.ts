// The display's grammar (ADR 0041, 전면 패널의 법칙 3): sections parted by
// hairlines, list rows, a marker on the chosen row, big numerals. Nothing in
// the display is a card, a raised button or a boxed field; every action is a
// row chosen from a list, and keyboard focus is a thin outline, never the
// chosen marker.

import type { Intent, ProfileKind } from '../../bindings';
import { type Child, h, svg } from '../../dom';
import { text } from '../../text';
import { type Band, bannerAction } from '../../screens/banner';
import type { Context } from '../../screens/common';
import { captureOf } from '../../screens/entrance';
import type { Frame } from '../controller';

/** A section with its small title, parted from the one before by a hairline. */
export function section(id: string, title: string | null, children: Child[], className = ''): HTMLElement {
  return h(
    'section',
    { class: `pn-sec ${className}`.trim(), 'aria-labelledby': title === null ? null : `${id}-title` },
    title === null ? null : h('h2', { id: `${id}-title`, class: 'pn-sec-title' }, title),
    ...children,
  );
}

/** A sentence of the display's quieter kind. */
export function hint(sentence: string, id: string | null = null, className = ''): HTMLParagraphElement {
  return h('p', { id, class: `pn-hint ${className}`.trim() }, sentence);
}

/** A sentence in the display's own white. */
export function line(sentence: Child, className = ''): HTMLParagraphElement {
  return h('p', { class: `pn-line ${className}`.trim() }, sentence);
}

/** Rows of facts: the name dim, the value in white. */
export function facts(rows: [string, Child][], className = ''): HTMLDListElement {
  const list = h('dl', { class: `pn-facts ${className}`.trim() });
  for (const [label, value] of rows) {
    list.append(h('div', { class: 'pn-fact' }, h('dt', null, label), h('dd', null, value)));
  }
  return list;
}

/** A path or a name the user gave, or 없음 in the quieter colour. */
export function given(value: string | null): HTMLElement {
  return h('span', { class: value === null ? 'pn-dim' : 'pn-path' }, value ?? text.profile.none);
}

/** A small lamp in a line: the state colours, or a lane's. */
export function dot(tone: 'on' | 'wait' | 'fault' | 'off' | 'white' | `lane-${number}`): HTMLSpanElement {
  return h('span', { class: `pn-dot is-${tone}`, 'aria-hidden': 'true' });
}

export interface RowOptions {
  /** Stable key so focus survives a redraw. */
  key: string;
  label: string;
  /** A quieter line under the label. */
  detail?: Child | undefined;
  disabled?: boolean | undefined;
  /** Its intent is in flight. */
  busy?: boolean | undefined;
  /** The label while busy. */
  busyLabel?: string | undefined;
  describedBy?: string | null | undefined;
  intent?: Intent['kind'] | undefined;
  /** For a row that opens a list under it. */
  expanded?: boolean | undefined;
  /** What the row stands at, at its end before the mark. */
  value?: string | undefined;
}

/** A menu row: its label, a quieter line, and the › that says it does something. */
export function row(options: RowOptions, activate: () => void): HTMLButtonElement {
  const label = options.busy && options.busyLabel !== undefined ? options.busyLabel : options.label;
  return h(
    'button',
    {
      type: 'button',
      class: 'pn-row',
      'data-key': options.key,
      'data-intent': options.intent ?? null,
      disabled: options.disabled ?? false,
      'aria-busy': options.busy ? 'true' : null,
      'aria-describedby': options.describedBy ?? null,
      'aria-expanded': options.expanded === undefined ? null : String(options.expanded),
      onClick: activate,
    },
    h(
      'span',
      { class: 'pn-row-text' },
      h('span', { class: 'pn-row-label' }, label),
      options.detail === undefined ? null : h('span', { class: 'pn-row-detail' }, options.detail),
    ),
    options.value === undefined ? null : h('span', { class: 'pn-row-value' }, options.value),
    h('span', { class: `pn-row-go${options.busy ? ' is-busy' : ''}`, 'aria-hidden': 'true' }, options.busy ? '' : '›'),
  );
}

/** A row that sends one intent; locked while any intent is in flight. */
export function intentRow(
  ctx: Context,
  label: string,
  intent: Intent,
  options: Omit<RowOptions, 'label' | 'intent' | 'busy'>,
): HTMLButtonElement {
  ctx.offer(intent.kind);
  return row(
    { ...options, label, intent: intent.kind, busy: ctx.pending === intent.kind, disabled: ctx.busy || (options.disabled ?? false) },
    () => ctx.send(intent),
  );
}

/** A row that asks the shell for a file or a folder and sends the path picked. */
export function pickRow(
  ctx: Context,
  label: string,
  key: string,
  locked: boolean,
  describedBy: string | null,
  pick: () => Promise<string | null>,
  send: (path: string) => void,
): HTMLButtonElement {
  return row({ key, label, disabled: ctx.busy || locked, describedBy }, () => {
    pick().then(
      (path) => {
        if (path !== null) send(path);
      },
      (reason: unknown) => ctx.report({ message: String(reason), next: 'none' }),
    );
  });
}

export interface Choice<T> {
  value: T;
  label: string;
  key: string;
  detail?: string | undefined;
  /** A short word after the label (미구현, 기본값). */
  tag?: string | undefined;
  disabled?: boolean | undefined;
  describedBy?: string | null | undefined;
  /** A lane colour beside the label. */
  lane?: number | undefined;
}

export interface ChoicesOptions<T> {
  label?: string | undefined;
  labelledBy?: string | undefined;
  describedBy?: string | null | undefined;
  options: Choice<T>[];
  chosen: T | null;
  disabled: boolean;
  busy: boolean;
  intent?: Intent['kind'] | undefined;
  /** Short options side by side instead of one per line. */
  inline?: boolean | undefined;
  /**
   * With `inline`, the options in lines of these many each, parted by
   * hairlines, as a receiver's menu lists one family of layouts per line.
   * The lines are only looks: the arrow keys still walk every option.
   */
  lines?: readonly number[] | undefined;
  choose(value: T): void;
}

/**
 * Layout names (`7.1.4`, `24.1.10`) in lines of one family each: ear level
 * only, with heights, the wide 9.1 family, then the layouts of 13 channels
 * and more. A family of one joins the line before it. The names keep their
 * order; the sizes are for `ChoicesOptions.lines`.
 */
export function layoutFamilies(names: readonly string[]): number[] {
  const family = (name: string) => {
    const parts = name.split('.');
    const bed = Number(parts[0]);
    return bed >= 13 ? 3 : bed >= 9 ? 2 : parts.length > 2 ? 1 : 0;
  };
  const lines: { family: number; size: number }[] = [];
  for (const name of names) {
    const last = lines.at(-1);
    if (last !== undefined && last.family === family(name)) last.size += 1;
    else lines.push({ family: family(name), size: 1 });
  }
  const merged: number[] = [];
  for (const line of lines) {
    if (line.size === 1 && merged.length > 0) merged[merged.length - 1]! += 1;
    else merged.push(line.size);
  }
  return merged;
}

/**
 * A list of options, the chosen one marked (radios). Arrow keys move the
 * focus along the list and Space or Enter chooses, so moving does not send.
 */
export function choices<T>(options: ChoicesOptions<T>): HTMLElement {
  const lined = options.inline === true && options.lines !== undefined;
  const group = h('div', {
    role: 'radiogroup',
    class: `pn-choices${options.inline ? ' is-inline' : ''}${lined ? ' is-lined' : ''}`,
    'aria-label': options.label ?? null,
    'aria-labelledby': options.labelledBy ?? null,
    'aria-describedby': options.describedBy ?? null,
    'aria-busy': options.busy ? 'true' : null,
  });
  const chosenAt = options.options.findIndex((option) => option.value === options.chosen);
  const buttons = options.options.map((option, index) => {
    const checked = index === chosenAt;
    return h(
      'button',
      {
        type: 'button',
        role: 'radio',
        class: `pn-row pn-choice${checked ? ' is-chosen' : ''}`,
        'data-key': option.key,
        'data-intent': options.intent ?? null,
        'aria-checked': checked ? 'true' : 'false',
        'aria-describedby': option.describedBy ?? null,
        tabindex: index === Math.max(chosenAt, 0) ? 0 : -1,
        disabled: options.disabled || (option.disabled ?? false),
        onClick: () => {
          if (!checked) options.choose(option.value);
        },
      },
      h('span', { class: 'pn-mark', 'aria-hidden': 'true' }),
      h(
        'span',
        { class: 'pn-row-text' },
        h(
          'span',
          { class: 'pn-row-label' },
          option.lane === undefined ? null : dot(`lane-${option.lane}`),
          option.label,
          option.tag === undefined ? null : h('span', { class: 'pn-tag' }, option.tag),
        ),
        option.detail === undefined ? null : h('span', { class: 'pn-row-detail' }, option.detail),
      ),
    );
  });
  if (lined) {
    let start = 0;
    for (const size of options.lines ?? []) {
      group.append(h('div', { class: 'pn-choice-line' }, ...buttons.slice(start, start + size)));
      start += size;
    }
    // Options the sizes do not reach go on a last line of their own.
    if (start < buttons.length) group.append(h('div', { class: 'pn-choice-line' }, ...buttons.slice(start)));
  } else {
    group.append(...buttons);
  }
  group.addEventListener('keydown', (event) => {
    const key = event.key;
    const enabled = buttons.filter((button) => !button.disabled);
    const at = enabled.indexOf(document.activeElement as HTMLButtonElement);
    if (at < 0 || enabled.length === 0) return;
    const forward = key === 'ArrowDown' || key === 'ArrowRight';
    const back = key === 'ArrowUp' || key === 'ArrowLeft';
    const to =
      forward ? (at + 1) % enabled.length
      : back ? (at - 1 + enabled.length) % enabled.length
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
  return group;
}

export interface ToggleOptions {
  key: string;
  label: string;
  on: boolean;
  disabled: boolean;
  busy: boolean;
  describedBy: string | null;
  toggle(on: boolean): void;
}

/** An on/off row (`role="switch"`): its name, and 켜짐 or 꺼짐 with a lamp at its end. */
export function toggle(options: ToggleOptions): HTMLButtonElement {
  return h(
    'button',
    {
      type: 'button',
      role: 'switch',
      id: options.key,
      class: `pn-row pn-toggle${options.on ? ' is-on' : ''}`,
      'data-key': options.key,
      'aria-checked': options.on ? 'true' : 'false',
      'aria-busy': options.busy ? 'true' : null,
      'aria-describedby': options.describedBy,
      disabled: options.disabled,
      onClick: () => options.toggle(!options.on),
    },
    h('span', { class: 'pn-row-text' }, h('span', { class: 'pn-row-label' }, options.label)),
    h(
      'span',
      { class: 'pn-row-value', 'aria-hidden': 'true' },
      options.on ? text.facts.on : text.facts.off,
      h('span', { class: `pn-dot is-${options.on ? 'white' : 'off'}` }),
    ),
  );
}

export interface LevelOptions {
  key: string;
  label: string;
  min: number;
  max: number;
  step: number;
  value: number;
  disabled: boolean;
  describedBy: string | null;
  /** The value as words, for the readout and `aria-valuetext`. */
  say(value: number): string;
  commit(value: number): void;
}

/**
 * A level on the display: a hairline track lit up to the value, its readout
 * beside it (`role="slider"`). Arrow keys and dragging move it; the value is
 * sent when the key or the pointer is let go.
 */
export function level(options: LevelOptions): HTMLElement {
  const { min, max, step } = options;
  let value = options.value;
  const span = max - min;
  const fill = h('span', { class: 'pn-level-fill' });
  const readout = h('span', { class: 'pn-level-readout num', 'aria-hidden': 'true' }, options.say(value));
  const slider = h(
    'div',
    {
      role: 'slider',
      class: 'pn-level',
      tabindex: 0,
      'data-key': options.key,
      'aria-label': options.label,
      'aria-valuemin': min,
      'aria-valuemax': max,
      'aria-valuenow': value,
      'aria-valuetext': options.say(value),
      'aria-disabled': options.disabled ? 'true' : null,
      'aria-describedby': options.describedBy,
    },
    h('span', { class: 'pn-level-track' }, fill),
  );
  const show = (next: number) => {
    value = Math.min(max, Math.max(min, Math.round(next / step) * step));
    fill.style.transform = `scaleX(${(value - min) / span})`;
    readout.textContent = options.say(value);
    slider.setAttribute('aria-valuenow', String(value));
    slider.setAttribute('aria-valuetext', options.say(value));
  };
  show(value);
  const commit = () => {
    if (value !== options.value) options.commit(value);
  };
  slider.addEventListener('keydown', (event) => {
    if (options.disabled) return;
    const moves: Record<string, number> = {
      ArrowRight: step,
      ArrowUp: step,
      ArrowLeft: -step,
      ArrowDown: -step,
      PageUp: step * 10,
      PageDown: -step * 10,
    };
    const by = moves[event.key];
    if (by !== undefined) show(value + by);
    else if (event.key === 'Home') show(min);
    else if (event.key === 'End') show(max);
    else return;
    event.preventDefault();
  });
  slider.addEventListener('keyup', (event) => {
    if (!options.disabled && ['ArrowRight', 'ArrowUp', 'ArrowLeft', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End'].includes(event.key)) {
      commit();
    }
  });
  const fromPointer = (event: PointerEvent) => {
    const box = slider.getBoundingClientRect();
    if (box.width > 0) show(min + ((event.clientX - box.left) / box.width) * span);
  };
  slider.addEventListener('pointerdown', (event) => {
    if (options.disabled) return;
    slider.setPointerCapture?.(event.pointerId);
    fromPointer(event);
  });
  slider.addEventListener('pointermove', (event) => {
    if (slider.hasPointerCapture?.(event.pointerId)) fromPointer(event);
  });
  slider.addEventListener('pointerup', (event) => {
    if (options.disabled) return;
    slider.releasePointerCapture?.(event.pointerId);
    commit();
  });
  return h('div', { class: 'pn-level-wrap' }, slider, readout);
}

/** The error bands: the sentence and the next action from Rust (ADR 0001). */
export function band(ctx: Context, frame: Frame, entry: Band, index: number): HTMLElement {
  const { error, kept } = entry;
  let action = bannerAction(ctx.snapshot.phase, error.next);
  // A refusal is not the step's explanation; it can always be closed.
  if (!kept && action.kind === 'nothing' && error.next === 'none') {
    action = { kind: 'dismiss', label: text.banner.dismiss };
  }
  let control: HTMLElement | null = null;
  const key = `banner-${index}`;
  switch (action.kind) {
    case 'intent':
      // The body or the chassis already offers it (the power button for a retry).
      if (!frame.offered.has(action.intent.kind)) control = intentRow(ctx, action.label, action.intent, { key });
      break;
    case 'choose_output':
      control = row({ key, label: action.label }, frame.chooseOutput);
      break;
    case 'dismiss':
      control = row({ key, label: action.label }, () => frame.dismiss(error));
      break;
    case 'nothing':
      break;
  }
  const warning = error.next === 'none' && ctx.snapshot.phase.kind === 'running';
  return h(
    'div',
    { class: `pn-band${warning ? ' is-warning' : ''}${kept ? '' : ' is-refusal'}`, role: 'alert', 'data-region': 'error-band' },
    dot(warning ? 'wait' : 'fault'),
    h(
      'div',
      { class: 'pn-band-text' },
      h('p', { class: 'pn-band-message' }, error.message),
      error.next === 'reboot' ? h('p', { class: 'pn-hint' }, text.banner.rebootHint) : null,
    ),
    control === null ? null : h('div', { class: 'pn-band-action' }, control),
  );
}

/** The snapshot's notes, as quiet lines. */
export function notes(lines: string[]): HTMLElement | null {
  if (lines.length === 0) return null;
  return h('ul', { class: 'pn-notes', 'aria-label': text.banner.notesLabel }, ...lines.map((note) => h('li', null, note)));
}

/** The outputs to choose from, the one in use marked; shared by the standby page and the wizard. */
export function outputRows(ctx: Context, keyPrefix: string): HTMLElement {
  const { outputs, chosen_output } = ctx.snapshot;
  if (outputs.length === 0) {
    return h(
      'div',
      { class: 'pn-outputs-empty' },
      hint(text.wizard.noOutputs),
      intentRow(ctx, text.banner.recheck, { kind: 'recheck' }, { key: `${keyPrefix}-recheck` }),
    );
  }
  ctx.offer('choose_output');
  const group = choices({
    label: text.wizard.outputsLabel,
    options: outputs.map((output, index) => ({
      value: output.id,
      label: output.name,
      key: `${keyPrefix}-${index}`,
      detail: output.id === chosen_output?.id ? text.status.currentOutput : undefined,
    })),
    chosen: chosen_output?.id ?? null,
    disabled: ctx.busy,
    busy: ctx.pending === 'choose_output',
    intent: 'choose_output',
    choose: (id) => ctx.send({ kind: 'choose_output', value: id }),
  });
  group.setAttribute('data-region', 'output-list');
  return group;
}

/**
 * The devices that can take the apps' original sound (ADR 0043), the one in
 * use marked; shared by the wizard's discard step and the settings menu.
 * With none, the wizard's band carries the supervisor's sentence and this is
 * the recheck row; elsewhere the sentence stands here. `locked` says why the
 * rows are dark (a running engine).
 */
export function discardRows(ctx: Context, keyPrefix: string, locked: string | null = null): HTMLElement {
  const { discards, discard } = captureOf(ctx.snapshot);
  if (discards.length === 0) {
    return ctx.snapshot.phase.kind === 'setup'
      ? h('div', { class: 'pn-outputs-empty' }, intentRow(ctx, text.banner.recheck, { kind: 'recheck' }, { key: `${keyPrefix}-recheck` }))
      : hint(text.entrance.noDiscards);
  }
  ctx.offer('choose_discard');
  const lockedId = `${keyPrefix}-locked`;
  const group = choices({
    label: text.wizard.discardsLabel,
    describedBy: locked === null ? null : lockedId,
    options: discards.map((output, index) => ({
      value: output.id,
      label: output.name,
      key: `${keyPrefix}-${index}`,
      detail: output.id === discard?.id ? text.status.currentOutput : undefined,
    })),
    chosen: discard?.id ?? null,
    disabled: ctx.busy || locked !== null,
    busy: ctx.pending === 'choose_discard',
    intent: 'choose_discard',
    choose: (id) => ctx.send({ kind: 'choose_discard', value: id }),
  });
  group.setAttribute('data-region', 'discard-list');
  return locked === null ? group : h('div', null, group, hint(locked, lockedId));
}

/** The output kind as a small line drawing: a headband or a speaker cone. */
export function kindIcon(kind: ProfileKind): SVGSVGElement {
  const stroke = { fill: 'none', stroke: 'currentColor', 'stroke-width': 1.5, 'stroke-linecap': 'round', 'stroke-linejoin': 'round' };
  return svg(
    'svg',
    { class: 'pn-kind-icon', viewBox: '0 0 20 20', width: 16, height: 16, 'aria-hidden': 'true', focusable: 'false' },
    ...(kind === 'headphones'
      ? [
          svg('path', { d: 'M3.5 12.5v-2a6.5 6.5 0 0 1 13 0v2', ...stroke }),
          svg('rect', { x: 2.75, y: 11.5, width: 3.5, height: 5.5, rx: 1.2, ...stroke }),
          svg('rect', { x: 13.75, y: 11.5, width: 3.5, height: 5.5, rx: 1.2, ...stroke }),
        ]
      : [
          svg('rect', { x: 4.5, y: 2.5, width: 11, height: 15, rx: 1.5, ...stroke }),
          svg('circle', { cx: 10, cy: 12, r: 3, ...stroke }),
          svg('circle', { cx: 10, cy: 6, r: 1, fill: 'currentColor' }),
        ]),
  );
}
