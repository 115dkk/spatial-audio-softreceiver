// Parts more than one face carries: the LEVEL encoder and the output kind's
// slide switch (status and profile), the rows of output devices and discard
// outputs (status, settings, setup), and each output kind's upmix switch
// (profile and upmix). Each sends what the standard screen sends.

import type { ProfileKind } from '../../bindings';
import { h } from '../../dom';
import { oneDecimal } from '../../format';
import { text } from '../../text';
import type { Context } from '../../screens/common';
import { captureOf } from '../../screens/entrance';
import { GAIN_MAX, GAIN_MIN } from '../../screens/profile';
import { encoder, fine, intentKey, keyRow, print, readout, slide } from './parts';
import { type Scope, profileOpen, signed } from './scope';

const KINDS: readonly ProfileKind[] = ['headphones', 'speakers'];
const GAIN_STEP = 0.5;
const FROM_FILE_ID = 'pd-kind-why-file';
const NEEDS_BRIR_ID = 'pd-kind-why-brir';

/**
 * The LEVEL encoder: the output gain (`set_gain`, the standard slider's range
 * and step) with its LED ring and the gain on a segment readout beside it.
 * The readout follows the knob while it turns; the gain is sent when it is
 * let go.
 */
export function levelEncoder(scope: Scope, size = 74): HTMLElement {
  const { ctx } = scope;
  const able = profileOpen(ctx.snapshot);
  if (able) ctx.offer('set_gain');
  const shown = readout(4, 30, text.profile.gainUnit, signed(scope.gain.value()), 'pd-gain-readout');
  scope.gain.follow((db) => shown.set(signed(db)));
  const knob = encoder({
    key: 'gain',
    label: text.profile.gainLabel,
    min: GAIN_MIN,
    max: GAIN_MAX,
    step: GAIN_STEP,
    settled: scope.gain.settled,
    value: scope.gain.value(),
    ring: 15,
    size,
    off: !able,
    locked: ctx.busy,
    busy: ctx.pending === 'set_gain',
    intent: able ? 'set_gain' : undefined,
    say: (db) => `${oneDecimal(db)} dB`,
    hold: scope.gain.hold,
    turned: (db) => scope.gain.turned(db),
    commit: (db) => {
      ctx.setView({ gainSending: db });
      ctx.send({ kind: 'set_gain', value: db });
    },
  });
  return h(
    'div',
    { class: `pd-level${able ? '' : ' is-off'}` },
    knob,
    h('div', { class: 'pd-level-say' }, print(text.status.volume), shown.element),
  );
}

/**
 * The output kind in use as a two-stop slide switch (`set_profile_kind`).
 * A stop that cannot be taken stays reachable and says why in print under
 * the switch: profile.toml decides, or the headphones have no BRIR set.
 */
export function kindSlide(scope: Scope): HTMLElement {
  const { ctx } = scope;
  const { profile } = ctx.snapshot;
  const able = profileOpen(ctx.snapshot);
  if (able) ctx.offer('set_profile_kind');
  const sending = ctx.sending?.kind === 'set_profile_kind' ? ctx.sending.value : null;
  const needsBrir = profile.headphones.brir === null && !profile.from_file;
  const refuse = (kind: ProfileKind) =>
    !able ? null
    : profile.from_file ? FROM_FILE_ID
    : kind === 'headphones' && needsBrir ? NEEDS_BRIR_ID
    : null;
  const control = slide<ProfileKind>({
    key: 'kind',
    label: text.profile.outputTitle,
    stops: KINDS.map((kind) => ({ value: kind, label: text.profile.kindNames[kind], key: `kind-${kind}` })),
    at: sending ?? profile.kind,
    off: !able,
    locked: ctx.busy,
    busy: sending !== null,
    intent: able ? 'set_profile_kind' : undefined,
    refuse,
    choose: (kind) => {
      if (kind !== profile.kind) ctx.send({ kind: 'set_profile_kind', value: kind });
    },
  });
  return h(
    'div',
    { class: 'pd-kind' },
    control,
    able && profile.from_file ? fine(text.profile.fromFile, FROM_FILE_ID) : null,
    able && needsBrir ? fine(text.profile.needsBrir, NEEDS_BRIR_ID) : null,
  );
}

/**
 * The output devices as a row of illuminated keys with their names printed,
 * the one in use lit (`data-region="output-list"`). With none, the sentence
 * and a recheck key.
 */
export function outputRow(ctx: Context, keyPrefix: string): HTMLElement {
  const { outputs, chosen_output } = ctx.snapshot;
  if (outputs.length === 0) {
    return h(
      'div',
      { class: 'pd-row-empty' },
      fine(text.wizard.noOutputs),
      intentKey(ctx, text.banner.recheck, { kind: 'recheck' }, { key: `${keyPrefix}-recheck` }),
    );
  }
  ctx.offer('choose_output');
  const row = keyRow({
    label: text.wizard.outputsLabel,
    // The ring of the output in use is lit (법칙 3); "current output" is for a screen reader, its label a plain string.
    choices: outputs.map((output, index) => ({
      value: output.id,
      label: output.name,
      key: `${keyPrefix}-${index}`,
      said: output.id === chosen_output?.id ? text.status.currentOutput : undefined,
    })),
    chosen: chosen_output?.id ?? null,
    off: false,
    locked: ctx.busy,
    busy: ctx.pending === 'choose_output',
    sending: ctx.sending?.kind === 'choose_output' ? ctx.sending.value : null,
    intent: 'choose_output',
    choose: (id) => ctx.send({ kind: 'choose_output', value: id }),
  });
  row.setAttribute('data-region', 'output-list');
  return row;
}

/**
 * The devices that can take the apps' original sound (ADR 0043) as a row of
 * keys, the one in use lit (`data-region="discard-list"`). With none, the
 * setup step's band carries the supervisor's sentence and this is the
 * recheck key; elsewhere the sentence is printed here. `lockedId` names the
 * note saying why the keys cannot be pressed (a running engine); the caller
 * stands the lock lamp by the row.
 */
export function discardRow(ctx: Context, keyPrefix: string, lockedId: string | null = null): HTMLElement {
  const { discards, discard } = captureOf(ctx.snapshot);
  if (discards.length === 0) {
    return ctx.snapshot.phase.kind === 'setup'
      ? h('div', { class: 'pd-row-empty' }, intentKey(ctx, text.banner.recheck, { kind: 'recheck' }, { key: `${keyPrefix}-recheck` }))
      : fine(text.entrance.noDiscards);
  }
  ctx.offer('choose_discard');
  const row = keyRow({
    label: text.wizard.discardsLabel,
    describedBy: lockedId,
    choices: discards.map((output, index) => ({
      value: output.id,
      label: output.name,
      key: `${keyPrefix}-${index}`,
      said: output.id === discard?.id ? text.status.currentOutput : undefined,
    })),
    chosen: discard?.id ?? null,
    // While the engine runs the keys cannot be pressed, but the one in use keeps its light (법칙 3).
    off: false,
    locked: ctx.busy || lockedId !== null,
    busy: ctx.pending === 'choose_discard',
    sending: ctx.sending?.kind === 'choose_discard' ? ctx.sending.value : null,
    intent: 'choose_discard',
    choose: (id) => ctx.send({ kind: 'choose_discard', value: id }),
  });
  row.setAttribute('data-region', 'discard-list');
  return row;
}

/**
 * Whether this output kind applies the upmixers (ADR 0035), as a two-stop
 * slide switch off and on; only while some input's upmixer is on.
 */
export function upmixApply(ctx: Context, kind: ProfileKind, label: string): HTMLElement | null {
  const { upmix } = ctx.snapshot;
  if (upmix.endpoints.every((endpoint) => endpoint.effective === 'off')) return null;
  ctx.offer('set_upmix_apply');
  const sending = ctx.sending?.kind === 'set_upmix_apply' && ctx.sending.value.kind === kind ? ctx.sending.value : null;
  const labelId = `upmix-apply-${kind}-label`;
  return h(
    'div',
    { class: 'pd-switchline' },
    print(label, 'pd-label', labelId),
    slide<boolean>({
      key: `upmix-apply-${kind}`,
      labelledBy: labelId,
      describedBy: 'upmix-apply-hint',
      stops: [
        { value: false, label: text.facts.off, key: `upmix-apply-${kind}-off` },
        { value: true, label: text.facts.on, key: `upmix-apply-${kind}-on` },
      ],
      at: sending?.on ?? upmix[kind],
      off: false,
      locked: ctx.busy,
      busy: sending !== null,
      intent: 'set_upmix_apply',
      choose: (on) => ctx.send({ kind: 'set_upmix_apply', value: { kind, on } }),
    }),
  );
}

/**
 * Layout names (`7.1.4`, `24.1.10`) in lines of one family each: ear level
 * only, with heights, the wide 9.1 family, then the layouts of 13 channels
 * and more. A family of one joins the line before it. The names keep their
 * order; the sizes are for `keyRow`'s `lines`.
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
