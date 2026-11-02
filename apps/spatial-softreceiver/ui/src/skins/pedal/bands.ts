// Errors and notes on the face (ADR 0046, 이펙터의 법칙 5): a printed
// warning field with a red LED (amber for a warning while running), the
// sentence from Rust, and its action as a momentary key. The snapshot's
// error stands near the top of the face; a refused intent's at its foot.
// Notes are printed lines, each with an amber LED.

import { h } from '../../dom';
import { text } from '../../text';
import { type Band, bannerAction } from '../../screens/banner';
import type { Context } from '../../screens/common';
import type { Frame } from '../controller';
import { fine, intentKey, led, mkey } from './parts';

/** One error band: the sentence and the next action from Rust (ADR 0001). */
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
      // The face or the box already offers it (the footswitch for a retry).
      if (!frame.offered.has(action.intent.kind)) control = intentKey(ctx, action.label, action.intent, { key });
      break;
    case 'choose_output':
      control = mkey({ key, label: action.label, onClick: frame.chooseOutput });
      break;
    case 'dismiss':
      control = mkey({ key, label: action.label, onClick: () => frame.dismiss(error) });
      break;
    case 'nothing':
      break;
  }
  const warning = error.next === 'none' && ctx.snapshot.phase.kind === 'running';
  return h(
    'div',
    { class: `pd-band${warning ? ' is-warning' : ''}${kept ? '' : ' is-refusal'}`, role: 'alert', 'data-region': 'error-band' },
    led(warning ? 'wait' : 'fault', 'on', 'md').element,
    h(
      'div',
      { class: 'pd-band-text' },
      h('p', { class: 'pd-band-message' }, error.message),
      error.next === 'reboot' ? fine(text.banner.rebootHint) : null,
    ),
    control === null ? null : h('div', { class: 'pd-band-action' }, control),
  );
}

/** The snapshot's notes, printed, an amber LED by each. */
export function notes(lines: string[]): HTMLElement | null {
  if (lines.length === 0) return null;
  return h(
    'ul',
    { class: 'pd-notes', 'aria-label': text.banner.notesLabel },
    ...lines.map((note) => h('li', null, led('wait', 'on', 'sm').element, h('span', null, note))),
  );
}
