// The upmix face (ADR 0046, ADR 0035): what upmixing does and to what,
// whether each output kind applies it (a slide switch each), then each
// input's upmixer: what it is doing now by its LED, the three upmixers as
// illuminated keys with the chosen one lit, the delay it adds, its VST
// plugin's facts printed with the momentary keys that pick it and open its
// editor. Every value comes from `Snapshot.upmix`; Rust decides whether a
// choice, a plugin or an editor is taken.

import type { EndpointUpmixView, PluginFormat, UpmixerChoice } from '../../bindings';
import { type Child, h } from '../../dom';
import { oneDecimal } from '../../format';
import { text } from '../../text';
import { type Context, type Drawn, described } from '../../screens/common';
import { bypassReason } from '../../screens/upmix';
import { upmixApply } from './controls';
import { facts, fine, flowRoom, group, keyRow, led, mkey, pickKey, print } from './parts';
import type { Scope } from './scope';

const CHOICES: readonly UpmixerChoice[] = ['first_party', 'plugin', 'off'];

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

/** What the input's upmixer is doing: its state LED and the words printed by it. */
function state(ctx: Context, view: EndpointUpmixView): HTMLElement {
  const { snapshot } = ctx;
  const says = (tone: 'on' | 'wait' | 'fault' | 'off', ...words: Child[]) =>
    h(
      'p',
      { class: `pd-upstate is-${tone}${view.state.kind === 'starting' ? ' is-starting' : ''}` },
      led(tone === 'off' ? 'on' : tone, tone === 'off' ? 'off' : 'on', 'md').element,
      h('span', null, ...words),
    );
  const detail = (words: string) => h('span', { class: 'pd-upstate-detail' }, words);
  switch (view.state.kind) {
    case 'stopped': {
      const idle = view.effective === 'off' || (view.effective === 'plugin' && view.plugin === null);
      if (idle || snapshot.phase.kind === 'running') return says('off', text.upmix.stateOff);
      // It starts with the engine; with the kind's switch off it will go around.
      const notApplied = !snapshot.upmix[snapshot.profile.kind];
      return says('off', text.upmix.stateAfterEnable, notApplied ? detail(text.upmix.bypassKindOff) : null);
    }
    case 'starting':
      return says('wait', text.upmix.stateStarting);
    case 'running': {
      const details: Child[] = [];
      if (view.bypass !== null) details.push(detail(bypassReason(view.bypass)));
      else if (view.latency_ms !== null) details.push(detail(text.upmix.latency(oneDecimal(view.latency_ms))));
      if (view.outputs.length > 0) details.push(detail(text.upmix.outputs(view.outputs.join(' '))));
      return says('on', text.upmix.stateRunning, ...details);
    }
    case 'fallback':
      return says('wait', text.upmix.stateFallback);
    case 'failed':
      return says('fault', view.state.value);
  }
}

/** The three upmixers as illuminated keys, the one the choice resolves to lit; the delay under them. */
function upmixer(ctx: Context, view: EndpointUpmixView): Child[] {
  const endpoint = view.endpoint;
  const built = ctx.snapshot.upmix.first_party_available;
  const sending = ctx.sending?.kind === 'set_upmixer' && ctx.sending.value.endpoint === endpoint ? ctx.sending.value : null;
  const delayId = `upmixer-${endpoint}-delay`;
  const applyingId = `upmixer-${endpoint}-applying`;
  const bodyId = `upmixer-${endpoint}-body`;
  const delay = view.effective !== 'off' && view.latency_ms !== null ? text.upmix.delayHint(oneDecimal(view.latency_ms)) : null;
  const shown = sending?.choice ?? view.effective;
  ctx.offer('set_upmixer');
  return [
    h('h3', { id: `upmixer-${endpoint}-label`, class: 'pd-label' }, text.upmix.choiceLabel),
    keyRow({
      labelledBy: `upmix-title-${endpoint} upmixer-${endpoint}-label`,
      describedBy: described(bodyId, delay !== null && delayId, sending !== null && applyingId),
      choices: CHOICES.map((choice) => ({
        value: choice,
        label: choiceName(choice),
        key: `upmixer-${endpoint}-${choice}`,
        detail:
          choice === 'first_party' && !built ? text.upmix.notBuilt
          : view.choice === null && choice === view.effective ? text.upmix.defaultNote
          : undefined,
        off: choice === 'first_party' && !built,
      })),
      chosen: shown,
      off: false,
      locked: ctx.busy,
      busy: sending !== null,
      intent: 'set_upmixer',
      choose: (choice) => ctx.send({ kind: 'set_upmixer', value: { endpoint, choice } }),
    }),
    h(
      'dl',
      { class: 'pd-facts pd-legend', id: bodyId },
      ...CHOICES.map((choice) => h('div', { class: 'pd-fact' }, h('dt', null, choiceName(choice)), h('dd', null, choiceBody(choice, built)))),
    ),
    delay === null ? null : fine(delay, delayId, 'pd-num'),
    sending === null ? null : fine(text.profile.applying, applyingId),
  ];
}

/** The VST upmixer's plugin: its facts printed, and the keys that pick and open it. */
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
          facts([
            [text.upmix.pluginName, chosen.name],
            [text.upmix.pluginVendor, chosen.vendor],
            [text.upmix.pluginFormat, formatName(chosen.format)],
            ...(view.inputs.length > 0 ? [[text.upmix.pluginInputs, view.inputs.join(' ')] as [string, Child]] : []),
            [text.upmix.pluginPath, h('span', { class: 'pd-path' }, chosen.path)],
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

function input(ctx: Context, view: EndpointUpmixView): HTMLElement {
  const endpoint = text.endpoints[view.endpoint];
  const name = text.pedal.lamps[view.endpoint] ?? endpoint?.name ?? String(view.endpoint);
  return h(
    'section',
    { class: 'pd-group pd-upmix-input', 'data-lane': view.endpoint, 'aria-labelledby': `upmix-title-${view.endpoint}` },
    h('h2', { id: `upmix-title-${view.endpoint}`, class: 'pd-tab' }, name),
    endpoint === undefined ? null : h('p', { class: 'pd-small' }, `${endpoint.name} · ${endpoint.purpose}`),
    state(ctx, view),
    ...upmixer(ctx, view),
    ...(view.effective === 'plugin' ? plugin(ctx, view) : []),
  );
}

export function upmixFace(scope: Scope): Drawn {
  const { ctx } = scope;
  const kinds = text.profile.kindNames;
  const headphones = upmixApply(ctx, 'headphones', kinds.headphones);
  const speakers = upmixApply(ctx, 'speakers', kinds.speakers);
  const element = h(
    'div',
    { class: 'pd-upmix' },
    h(
      'div',
      { class: 'pd-upmix-head' },
      h(
        'div',
        { class: 'pd-upmix-about' },
        h('p', { class: 'pd-lead' }, text.upmix.intro),
        // The standard screen's (i) line, printed outright: the box has no pop-ups.
        h('p', { class: 'pd-fine', id: 'upmix-info' }, print(text.upmix.infoLabel, 'pd-label'), ' ', text.upmix.infoTip),
      ),
      headphones === null || speakers === null
        ? null
        : group('pd-upmix-apply', text.profile.upmixApply, [headphones, speakers, fine(text.profile.upmixApplyHint, 'upmix-apply-hint')]),
    ),
    h('div', { class: 'pd-upmix-inputs' }, ...ctx.snapshot.upmix.endpoints.map((view) => input(ctx, view))),
    flowRoom(),
  );
  return { element };
}
