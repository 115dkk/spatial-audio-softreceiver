// The upmix page in the display (ADR 0035): what upmixing does and to what,
// whether each output kind applies it, then each input's upmixer as a list
// to choose from, the delay it adds, its VST plugin and what it is doing now.
// Every value comes from `Snapshot.upmix`; Rust decides whether a choice, a
// plugin or an editor is taken.

import type { EndpointUpmixView, PluginFormat, UpmixerChoice } from '../../bindings';
import { type Child, h } from '../../dom';
import { oneDecimal } from '../../format';
import { text } from '../../text';
import { type Context, type Drawn, described } from '../../screens/common';
import { bypassReason } from '../../screens/upmix';
import { upmixApply } from './profile';
import type { Scope } from './scope';
import { choices, dot, facts, hint, line, pickRow, row, section } from './ui';

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

/** What the input's upmixer is doing, in one line with its lamp. */
function state(ctx: Context, view: EndpointUpmixView): HTMLElement {
  const { snapshot } = ctx;
  const says = (tone: 'on' | 'wait' | 'fault' | 'off', ...words: Child[]) =>
    h('p', { class: `pn-upstate is-${tone}` }, tone === 'wait' && view.state.kind === 'starting' ? h('span', { class: 'pn-busy', 'aria-hidden': 'true' }) : dot(tone), h('span', null, ...words));
  const detail = (words: string) => h('span', { class: 'pn-upstate-detail' }, words);
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

/** The three upmixers as a list, the one the choice resolves to marked; the delay under it. */
function upmixer(ctx: Context, view: EndpointUpmixView): Child[] {
  const endpoint = view.endpoint;
  const built = ctx.snapshot.upmix.first_party_available;
  const sending = ctx.sending?.kind === 'set_upmixer' && ctx.sending.value.endpoint === endpoint ? ctx.sending.value : null;
  const delayId = `upmixer-${endpoint}-delay`;
  const applyingId = `upmixer-${endpoint}-applying`;
  const delay = view.effective !== 'off' && view.latency_ms !== null ? text.upmix.delayHint(oneDecimal(view.latency_ms)) : null;
  ctx.offer('set_upmixer');
  return [
    h('h3', { id: `upmixer-${endpoint}-label`, class: 'pn-sub-title' }, text.upmix.choiceLabel),
    choices({
      labelledBy: `upmix-title-${endpoint} upmixer-${endpoint}-label`,
      describedBy: described(delay !== null && delayId, sending !== null && applyingId),
      options: CHOICES.map((choice) => ({
        value: choice,
        label: choiceName(choice),
        key: `upmixer-${endpoint}-${choice}`,
        detail: choiceBody(choice, built),
        tag: choice === 'first_party' && !built ? text.upmix.notBuilt : view.choice === null && choice === view.effective ? text.upmix.defaultNote : undefined,
        disabled: choice === 'first_party' && !built,
      })),
      chosen: sending?.choice ?? view.effective,
      disabled: ctx.busy,
      busy: sending !== null,
      intent: 'set_upmixer',
      choose: (choice) => ctx.send({ kind: 'set_upmixer', value: { endpoint, choice } }),
    }),
    delay === null ? null : hint(delay, delayId, 'num'),
    sending === null ? null : hint(text.profile.applying, applyingId),
  ];
}

/** The VST upmixer's plugin: what and where it is, and the rows that choose and open it. */
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
    editor = row(
      {
        key: `plugin-editor-${endpoint}`,
        label: text.upmix.openEditor,
        intent: 'open_plugin_editor',
        // Another input's editor in flight is not this one's.
        busy: mine && ctx.pending === 'open_plugin_editor',
        busyLabel: text.upmix.opening,
        disabled: ctx.busy || !editorOpen(view),
        describedBy: editorOpen(view) ? null : editorId,
      },
      () => ctx.send({ kind: 'open_plugin_editor', value: endpoint }),
    );
  }

  return [
    h('h3', { id: `plugin-${endpoint}-title`, class: 'pn-sub-title' }, text.upmix.pluginTitle),
    ...(chosen === null
      ? [line(text.upmix.noPlugin), hint(text.upmix.needsPlugin, neededId)]
      : [
          facts([
            [text.upmix.pluginName, chosen.name],
            [text.upmix.pluginVendor, chosen.vendor],
            [text.upmix.pluginFormat, formatName(chosen.format)],
            ...(view.inputs.length > 0 ? [[text.upmix.pluginInputs, view.inputs.join(' ')] as [string, Child]] : []),
            [text.upmix.pluginPath, h('span', { class: 'pn-path' }, chosen.path)],
          ]),
        ]),
    pickRow(ctx, text.upmix.pickFolder, `plugin-folder-${endpoint}`, false, pickIds, () => ctx.shell.pickFolder(text.upmix.folderDialog), send),
    pickRow(
      ctx,
      text.upmix.pickFile,
      `plugin-file-${endpoint}`,
      false,
      pickIds,
      () => ctx.shell.pickFile(text.upmix.fileDialog, { name: text.upmix.fileFilter, extensions: ['vst3', 'dll'] }),
      send,
    ),
    editor,
    scanning ? hint(text.upmix.scanning, scanId) : null,
    editor !== null && !editorOpen(view) ? hint(text.upmix.editorHint, editorId) : null,
  ];
}

function input(ctx: Context, view: EndpointUpmixView): HTMLElement {
  const endpoint = text.endpoints[view.endpoint];
  const name = text.panel.lamps[view.endpoint] ?? endpoint?.name ?? String(view.endpoint);
  return h(
    'section',
    { class: 'pn-sec pn-upmix-input', 'data-lane': view.endpoint, 'aria-labelledby': `upmix-title-${view.endpoint}` },
    h('h2', { id: `upmix-title-${view.endpoint}`, class: 'pn-sec-title pn-lane-title' }, dot(`lane-${view.endpoint}`), name),
    endpoint === undefined ? null : hint(`${endpoint.name} · ${endpoint.purpose}`),
    state(ctx, view),
    ...upmixer(ctx, view),
    ...(view.effective === 'plugin' ? plugin(ctx, view) : []),
  );
}

export function upmixPage(scope: Scope): Drawn {
  const { ctx } = scope;
  const kinds = text.profile.kindNames;
  const headphones = upmixApply(ctx, 'headphones', kinds.headphones);
  const speakers = upmixApply(ctx, 'speakers', kinds.speakers);
  const element = h(
    'div',
    { class: 'pn-upmix' },
    section('pn-upmix-about', null, [
      line(text.upmix.intro),
      // The standard screen's (i) line, said outright: the display has no pop-ups.
      h('p', { class: 'pn-hint', id: 'upmix-info' }, h('span', { class: 'pn-hint-head' }, text.upmix.infoLabel), ' ', text.upmix.infoTip),
    ]),
    headphones === null || speakers === null
      ? null
      : section('pn-upmix-apply', text.profile.upmixApply, [headphones, speakers, hint(text.profile.upmixApplyHint, 'upmix-apply-hint')]),
    h('div', { class: 'pn-upmix-inputs' }, ...ctx.snapshot.upmix.endpoints.map((view) => input(ctx, view))),
  );
  return { element };
}
