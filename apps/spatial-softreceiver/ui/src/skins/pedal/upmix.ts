// The upmix face (ADR 0046, ADR 0035): what upmixing does and to what in a
// tabbed legend box, whether each output kind applies it (a slide switch
// each), then each input's upmixer in its own frame, its sections parted by
// printed hairlines: what it is doing now by its LED (a caution's sentence
// on the face's legend plate by its number) and, while it runs, which
// channels it puts out on a channel indicator strip; the three upmixers as
// illuminated keys with the chosen one lit and only its description printed
// in a caption the lit key's leader runs down to; the delay it adds on a
// segment readout; its VST plugin's facts on a spec plate with the momentary
// keys that pick it and open its editor. Every value comes from
// `Snapshot.upmix`; Rust decides whether a choice, a plugin or an editor is
// taken.

import type { EndpointUpmixView, PluginFormat, UpmixerChoice } from '../../bindings';
import { type Child, h } from '../../dom';
import { oneDecimal } from '../../format';
import { text } from '../../text';
import { type Context, type Drawn, described } from '../../screens/common';
import { bypassReason } from '../../screens/upmix';
import { upmixApply } from './controls';
import {
  type ChannelSpot,
  type Glow,
  type NotePlate,
  type Tone,
  channelStrip,
  fine,
  flowRoom,
  group,
  hairline,
  keyRow,
  led,
  mkey,
  notePlate,
  pickKey,
  print,
  readout,
  spec,
  sr,
} from './parts';
import type { Scope } from './scope';

const CHOICES: readonly UpmixerChoice[] = ['first_party', 'plugin', 'off'];

/**
 * The positions the output indicator prints, as a processor's panel does:
 * the 5.1 and 7.1 beds (names as the contract gives them, front to back),
 * then the four heights. An upmixer's channel lights the position of the same
 * name; one with a name not here gets a position of its own after them.
 */
const BED = ['L', 'C', 'R', 'LFE', 'Ls', 'Rs', 'Lss', 'Rss', 'Lrs', 'Rrs'];
const HEIGHTS = ['Tfl', 'Tfr', 'Trl', 'Trr'];

/** The input an upmix intent in flight is for, or null. */
function sendingFor(ctx: Context): number | null {
  const intent = ctx.sending;
  if (intent === null) return null;
  switch (intent.kind) {
    case 'set_upmixer':
    case 'choose_plugin':
      return intent.value.endpoint;
    case 'open_plugin_editor':
      return intent.value;
    default:
      return null;
  }
}

function choiceName(choice: UpmixerChoice): string {
  switch (choice) {
    case 'first_party':
      return text.upmix.firstParty;
    case 'plugin':
      return text.upmix.plugin;
    case 'off':
      return text.upmix.off;
  }
}

function choiceBody(choice: UpmixerChoice, built: boolean): string {
  switch (choice) {
    case 'first_party':
      return built ? text.upmix.firstPartyBody : `${text.upmix.firstPartyBody} ${text.upmix.firstPartyHint}`;
    case 'plugin':
      return text.upmix.pluginBody;
    case 'off':
      return text.upmix.offBody;
  }
}

function formatName(format: PluginFormat): string {
  return format === 'vst3' ? text.upmix.formatNames.vst3 : text.upmix.formatNames.vst2;
}

/** The editor opens only while the plugin's host process is up. */
const editorOpen = (view: EndpointUpmixView) => view.state.kind === 'running' || view.state.kind === 'fallback';

/** Which channels the running upmixer puts out, on the processor's indicator positions. */
function outputs(view: EndpointUpmixView): HTMLElement {
  const extra = view.outputs.filter((name) => !BED.includes(name) && !HEIGHTS.includes(name));
  const spots: ChannelSpot[] = [...BED, ...HEIGHTS, ...extra].map((name) => ({ name }));
  const strip = channelStrip(spots, { lines: [BED.length, HEIGHTS.length] });
  strip.set((_, name) => view.outputs.includes(name), text.upmix.outputs(view.outputs.join(' ')));
  return h('div', { class: 'pd-upout' }, print(text.status.output, 'pd-label pd-upout-label'), strip.element);
}

/**
 * What the input's upmixer is doing: its state LED and the short word printed
 * by it. A sentence about it (why the sound goes around it, why it stopped)
 * is the legend plate's, by its number.
 */
function state(ctx: Context, view: EndpointUpmixView, plate: NotePlate, source: string): Child[] {
  const { snapshot } = ctx;
  const says = (tone: 'on' | 'wait' | 'fault' | 'off', word: string | null, note: string | null = null) => {
    const colour: Tone = tone === 'off' ? 'on' : tone;
    const glow: Glow = tone === 'off' ? 'off' : 'on';
    let mark: HTMLElement | null = null;
    if (note !== null) {
      const entry = plate.note(colour, { led: false, glow, source });
      entry.set(note);
      mark = entry.mark;
    }
    return h(
      'p',
      { class: `pd-upstate is-${tone}${view.state.kind === 'starting' ? ' is-starting' : ''}` },
      led(colour, glow, 'md').element,
      word === null ? null : h('span', null, word),
      mark,
    );
  };
  switch (view.state.kind) {
    case 'stopped': {
      const idle = view.effective === 'off' || (view.effective === 'plugin' && view.plugin === null);
      if (idle || snapshot.phase.kind === 'running') return [says('off', text.upmix.stateOff)];
      // It starts with the engine; with the kind's switch off it will go around.
      const notApplied = !snapshot.upmix[snapshot.profile.kind];
      return [says('off', text.upmix.stateAfterEnable, notApplied ? text.upmix.bypassKindOff : null)];
    }
    case 'starting':
      return [says('wait', text.upmix.stateStarting)];
    case 'running':
      return [
        says('on', text.upmix.stateRunning, view.bypass === null ? null : bypassReason(view.bypass)),
        view.outputs.length > 0 ? outputs(view) : null,
      ];
    case 'fallback':
      return [says('wait', null, text.upmix.stateFallback)];
    case 'failed':
      return [says('fault', null, view.state.value)];
  }
}

/**
 * The three upmixers as illuminated keys, the one the choice resolves to lit,
 * and under them only that one's description, in a printed caption the lit
 * key's leader runs down to. The others' descriptions stay with their keys
 * for a screen reader.
 */
function upmixer(ctx: Context, view: EndpointUpmixView): Child[] {
  const endpoint = view.endpoint;
  const built = ctx.snapshot.upmix.first_party_available;
  const sending = ctx.sending?.kind === 'set_upmixer' && ctx.sending.value.endpoint === endpoint ? ctx.sending.value : null;
  const applyingId = `upmixer-${endpoint}-applying`;
  const bodyId = `upmixer-${endpoint}-body`;
  const delayId = `upmixer-${endpoint}-delay`;
  const delays = view.effective !== 'off' && view.latency_ms !== null;
  const shown = sending?.choice ?? view.effective;
  const aboutId = (choice: UpmixerChoice) => `upmixer-${endpoint}-${choice}-about`;
  ctx.offer('set_upmixer');
  const row = keyRow({
    labelledBy: `upmix-title-${endpoint} upmixer-${endpoint}-label`,
    describedBy: described(bodyId, delays && delayId, sending !== null && applyingId),
    choices: CHOICES.map((choice) => ({
      value: choice,
      label: choiceName(choice),
      key: `upmixer-${endpoint}-${choice}`,
      detail:
        choice === 'first_party' && !built ? text.upmix.notBuilt
        : view.choice === null && choice === view.effective ? text.upmix.defaultNote
        : undefined,
      off: choice === 'first_party' && !built,
      describedBy: choice === shown ? null : aboutId(choice),
    })),
    chosen: shown,
    off: false,
    locked: ctx.busy,
    busy: sending !== null,
    intent: 'set_upmixer',
    choose: (choice) => ctx.send({ kind: 'set_upmixer', value: { endpoint, choice } }),
  });
  row.classList.add('pd-upkeys', 'pd-leads');
  return [
    h('h3', { id: `upmixer-${endpoint}-label`, class: 'pd-label' }, text.upmix.choiceLabel),
    row,
    h('p', { class: 'pd-callout', id: bodyId, 'data-chosen': shown }, choiceBody(shown, built)),
    ...CHOICES.filter((choice) => choice !== shown).map((choice) => sr(choiceBody(choice, built), aboutId(choice))),
    sending === null ? null : fine(text.profile.applying, applyingId),
  ];
}

/** The delay upmixing adds to this input, on a segment readout; its sentence for a screen reader. */
function delay(view: EndpointUpmixView): HTMLElement | null {
  if (view.effective === 'off' || view.latency_ms === null) return null;
  const ms = oneDecimal(view.latency_ms);
  const digits = readout(4, 18, text.meters.ms, ms);
  // The printed name and the digits are for the eye; the sentence says both.
  digits.set(ms, '');
  return h(
    'div',
    { class: 'pd-updelay' },
    h('span', { class: 'pd-label', 'aria-hidden': 'true' }, text.status.latency),
    h('span', { 'aria-hidden': 'true' }, digits.element),
    sr(text.upmix.delayHint(ms), `upmixer-${view.endpoint}-delay`),
  );
}

/** The VST upmixer's plugin: its facts on a spec plate, and the keys that pick and open it. */
function plugin(ctx: Context, view: EndpointUpmixView): Child[] {
  const endpoint = view.endpoint;
  const chosen = view.plugin;
  const mine = sendingFor(ctx) === endpoint;
  const scanning = mine && ctx.pending === 'choose_plugin';
  const scanId = `plugin-${endpoint}-scanning`;
  const neededId = `plugin-${endpoint}-needed`;
  const editorId = `plugin-${endpoint}-editor-hint`;
  const pickIds = described(chosen === null && neededId, scanning && scanId);
  const send = (path: string) => ctx.send({ kind: 'choose_plugin', value: { endpoint, path } });
  ctx.offer('choose_plugin');

  let editor: HTMLButtonElement | null = null;
  if (chosen !== null) {
    ctx.offer('open_plugin_editor');
    const busy = mine && ctx.pending === 'open_plugin_editor';
    editor = mkey({
      key: `plugin-editor-${endpoint}`,
      label: busy ? text.upmix.opening : text.upmix.openEditor,
      intent: 'open_plugin_editor',
      busy,
      off: !editorOpen(view),
      locked: ctx.busy,
      describedBy: editorOpen(view) ? null : editorId,
      onClick: () => ctx.send({ kind: 'open_plugin_editor', value: endpoint }),
    });
  }

  return [
    h('h3', { id: `plugin-${endpoint}-title`, class: 'pd-label' }, text.upmix.pluginTitle),
    ...(chosen === null
      ? [h('p', { class: 'pd-small' }, text.upmix.noPlugin), fine(text.upmix.needsPlugin, neededId)]
      : [
          spec([
            [text.upmix.pluginName, chosen.name],
            [text.upmix.pluginVendor, chosen.vendor],
            [text.upmix.pluginFormat, formatName(chosen.format)],
            ...(view.inputs.length > 0 ? [[text.upmix.pluginInputs, view.inputs.join(' ')] as [string, Child]] : []),
            [text.upmix.pluginPath, h('span', { class: 'pd-path' }, chosen.path), true],
          ]),
        ]),
    h(
      'div',
      { class: 'pd-keys' },
      pickKey(ctx, text.upmix.pickFolder, `plugin-folder-${endpoint}`, false, pickIds, () => ctx.shell.pickFolder(text.upmix.folderDialog), send),
      pickKey(
        ctx,
        text.upmix.pickFile,
        `plugin-file-${endpoint}`,
        false,
        pickIds,
        () => ctx.shell.pickFile(text.upmix.fileDialog, { name: text.upmix.fileFilter, extensions: ['vst3', 'dll'] }),
        send,
      ),
      editor,
    ),
    scanning ? fine(text.upmix.scanning, scanId) : null,
    editor !== null && !editorOpen(view) ? fine(text.upmix.editorHint, editorId) : null,
  ];
}

function input(ctx: Context, view: EndpointUpmixView, plate: NotePlate): HTMLElement {
  const endpoint = text.endpoints[view.endpoint];
  const name = text.pedal.lamps[view.endpoint] ?? endpoint?.name ?? String(view.endpoint);
  const delayLine = delay(view);
  return h(
    'section',
    { class: 'pd-group pd-upmix-input', 'data-lane': view.endpoint, 'aria-labelledby': `upmix-title-${view.endpoint}` },
    h('h2', { id: `upmix-title-${view.endpoint}`, class: 'pd-tab' }, name),
    endpoint === undefined ? null : h('p', { class: 'pd-small' }, `${endpoint.name} · ${endpoint.purpose}`),
    ...state(ctx, view, plate, name),
    hairline(),
    ...upmixer(ctx, view),
    delayLine === null ? null : hairline(),
    delayLine,
    ...(view.effective === 'plugin' ? [hairline(), ...plugin(ctx, view)] : []),
  );
}

export function upmixFace(scope: Scope): Drawn {
  const { ctx } = scope;
  const kinds = text.profile.kindNames;
  const headphones = upmixApply(ctx, 'headphones', kinds.headphones);
  const speakers = upmixApply(ctx, 'speakers', kinds.speakers);
  const plate = notePlate(text.banner.notesLabel, 'pd-upmix-notes');
  const inputs = ctx.snapshot.upmix.endpoints.map((view) => input(ctx, view, plate));
  plate.renumber();
  const element = h(
    'div',
    { class: 'pd-upmix' },
    h(
      'div',
      { class: 'pd-upmix-head' },
      group('pd-upmix-about', text.upmix.title, [
        h('p', { class: 'pd-lead' }, text.upmix.intro),
        hairline(),
        // The standard screen's (i) line, printed outright: the box has no pop-ups.
        h('p', { class: 'pd-fine pd-upmix-info', id: 'upmix-info' }, print(text.upmix.infoLabel, 'pd-label'), ' ', text.upmix.infoTip),
      ]),
      headphones === null || speakers === null
        ? null
        : group('pd-upmix-apply', text.profile.upmixApply, [headphones, speakers, hairline(), fine(text.profile.upmixApplyHint, 'upmix-apply-hint')]),
    ),
    h('div', { class: 'pd-upmix-inputs' }, ...inputs),
    plate.element,
    flowRoom(),
  );
  return { element };
}
