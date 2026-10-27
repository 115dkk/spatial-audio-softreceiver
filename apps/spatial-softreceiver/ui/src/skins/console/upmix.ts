// The upmix page (ADR 0035): what upmixing does, every entrance's upmixer in
// one table, whether each output kind applies them, and per entrance its
// choice and its VST plugin as label | control rows. Rust decides whether a
// choice, a plugin or an editor is taken.

import type { EndpointUpmixView, PluginFormat, UpmixerChoice } from '../../bindings';
import { type Child, h } from '../../dom';
import { oneDecimal } from '../../format';
import { described, infoTip } from '../../screens/common';
import { text } from '../../text';
import { type Build, act, badge, cls, entrance, hint, laneMark, para, pick, row, section, table } from './kit';
import { type Page, choiceName, upmixState } from './overview';
import { upmixApply } from './profile';

const CHOICES: readonly UpmixerChoice[] = ['first_party', 'plugin', 'off'];

function choiceBody(choice: UpmixerChoice, built: boolean): string {
  const u = text.upmix;
  switch (choice) {
    case 'first_party':
      return built ? u.firstPartyBody : `${u.firstPartyBody} ${u.firstPartyHint}`;
    case 'plugin':
      return u.pluginBody;
    case 'off':
      return u.offBody;
  }
}

function formatName(format: PluginFormat): string {
  return format === 'vst3' ? text.upmix.formatNames.vst3 : text.upmix.formatNames.vst2;
}

/** The endpoint an upmix intent in flight is for. */
function sendingFor(b: Build): number | null {
  const intent = b.ctx.sending;
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

const editorOpen = (view: EndpointUpmixView) => view.state.kind === 'running' || view.state.kind === 'fallback';

function summary(b: Build): HTMLElement {
  const { snapshot } = b.ctx;
  const u = text.upmix;
  const rows = snapshot.upmix.endpoints.map((view) => {
    const state = upmixState(snapshot, view);
    const delay = view.effective !== 'off' && view.latency_ms !== null ? `${oneDecimal(view.latency_ms)} ${text.meters.ms}` : '-';
    return h(
      'tr',
      { 'data-endpoint': view.endpoint },
      h('th', { scope: 'row', class: 'cn-cell-name' }, h('span', { class: 'cn-entrance' }, laneMark(view.endpoint), h('span', null, entrance(view.endpoint).name))),
      h(
        'td',
        null,
        choiceName(view.effective),
        view.choice === null ? h('span', { class: 'cn-cell-sub' }, u.defaultNote) : null,
      ),
      h('td', { class: 'cn-cell-state' }, badge(state.tone, state.word)),
      h('td', { class: 'cn-num cn-cell-num' }, delay),
      h('td', { class: 'cn-muted' }, view.outputs.length > 0 ? view.outputs.join(' ') : '-'),
      h('td', null, view.plugin?.name ?? '-'),
    );
  });
  return table({
    labelledBy: 'cn-sec-upmix-all-title',
    head: [text.status.entrances, u.choiceLabel, text.status.stateLabel, text.console.delay, text.console.upmixOutputs, u.pluginTitle],
    rows,
  });
}

/** The three upmixers as radios, each with what it does under its name. */
function choices(b: Build, view: EndpointUpmixView, titleId: string): HTMLElement {
  const { ctx } = b;
  const endpoint = view.endpoint;
  const built = ctx.snapshot.upmix.first_party_available;
  const sending = ctx.sending?.kind === 'set_upmixer' && ctx.sending.value.endpoint === endpoint ? ctx.sending.value : null;
  const shown = sending?.choice ?? view.effective;
  const labelId = `upmixer-${endpoint}-label`;
  const applyingId = `upmixer-${endpoint}-applying`;
  const delayId = `upmixer-${endpoint}-delay`;
  const delay = view.effective !== 'off' && view.latency_ms !== null ? text.upmix.delayHint(oneDecimal(view.latency_ms)) : null;
  const group = h('div', {
    class: 'cn-radios',
    role: 'radiogroup',
    'aria-labelledby': `${titleId} ${labelId}`,
    'aria-describedby': delay === null ? null : delayId,
    'aria-busy': sending !== null ? 'true' : null,
  });
  for (const choice of CHOICES) {
    const key = `upmixer-${endpoint}-${choice}`;
    const nameId = `${key}-name`;
    const bodyId = `${key}-hint`;
    const unbuilt = choice === 'first_party' && !built;
    const checked = choice === shown;
    const reason = unbuilt ? text.upmix.notBuilt : ctx.busy ? text.console.busy : checked ? text.console.current : null;
    const input = h('input', {
      type: 'radio',
      id: key,
      name: `upmixer-${endpoint}`,
      value: choice,
      'data-key': key,
      'data-intent': 'set_upmixer',
      checked,
      disabled: ctx.busy || unbuilt,
      'aria-labelledby': nameId,
      'aria-describedby': described(bodyId, sending !== null && applyingId),
    });
    input.addEventListener('change', () => {
      if (input.checked) ctx.send({ kind: 'set_upmixer', value: { endpoint, choice } });
    });
    b.commands.push({
      id: key,
      label: `${text.upmix.choiceLabel}: ${choiceName(choice)}`,
      group: entrance(endpoint).name,
      action: null,
      disabled: reason !== null,
      reason,
      run: () => ctx.send({ kind: 'set_upmixer', value: { endpoint, choice } }),
    });
    group.append(
      h(
        'label',
        { class: cls('cn-radio', checked && 'is-checked', unbuilt && 'is-locked') },
        input,
        h(
          'span',
          { class: 'cn-radio-text' },
          h(
            'span',
            { id: nameId, class: 'cn-radio-name' },
            choiceName(choice),
            unbuilt ? h('span', { class: 'cn-tag' }, text.upmix.notBuilt) : null,
            view.choice === null && choice === view.effective ? h('span', { class: 'cn-tag' }, text.upmix.defaultNote) : null,
          ),
          h('span', { id: bodyId, class: 'cn-radio-body' }, choiceBody(choice, built)),
        ),
      ),
    );
  }
  return row(h('span', { id: labelId }, text.upmix.choiceLabel), [
    group,
    delay === null ? null : hint(delay, delayId, 'cn-num'),
    sending === null ? null : hint(text.profile.applying, applyingId),
  ]);
}

function plugin(b: Build, view: EndpointUpmixView): HTMLElement {
  const { ctx } = b;
  const u = text.upmix;
  const endpoint = view.endpoint;
  const mine = sendingFor(b) === endpoint;
  const scanning = mine && ctx.pending === 'choose_plugin';
  const scanId = `plugin-${endpoint}-scanning`;
  const neededId = `plugin-${endpoint}-needed`;
  const editorId = `plugin-${endpoint}-editor-hint`;
  const pickIds = described(view.plugin === null && neededId, scanning && scanId);
  const send = (path: string) => ctx.send({ kind: 'choose_plugin', value: { endpoint, path } });
  const about: Child[] =
    view.plugin === null
      ? [para(u.noPlugin), hint(u.needsPlugin, neededId)]
      : [
          h(
            'dl',
            { class: 'cn-facts' },
            ...(
              [
                [u.pluginName, view.plugin.name],
                [u.pluginVendor, view.plugin.vendor],
                [u.pluginFormat, formatName(view.plugin.format)],
                ...(view.inputs.length > 0 ? [[u.pluginInputs, view.inputs.join(' ')] as const] : []),
                [u.pluginPath, h('span', { class: 'cn-path' }, view.plugin.path)],
              ] as const
            ).map(([label, value]) => h('div', null, h('dt', null, label), h('dd', null, value))),
          ),
        ];
  const editor =
    view.plugin === null
      ? null
      : act(b, u.openEditor, { kind: 'open_plugin_editor', value: endpoint }, {
          key: `plugin-editor-${endpoint}`,
          mine,
          pendingLabel: u.opening,
          locked: editorOpen(view) ? null : u.editorHint,
          describedBy: editorOpen(view) ? null : editorId,
        });
  return row(u.pluginTitle, [
    ...about,
    h(
      'div',
      { class: 'cn-actions' },
      pick(b, u.pickFolder, `plugin-folder-${endpoint}`, null, pickIds, () => ctx.shell.pickFolder(u.folderDialog), send),
      pick(
        b,
        u.pickFile,
        `plugin-file-${endpoint}`,
        null,
        pickIds,
        () => ctx.shell.pickFile(u.fileDialog, { name: u.fileFilter, extensions: ['vst3', 'dll'] }),
        send,
      ),
      editor,
    ),
    scanning ? hint(u.scanning, scanId) : null,
    editor !== null && !editorOpen(view) ? hint(u.editorHint, editorId) : null,
  ]);
}

function endpointSection(b: Build, view: EndpointUpmixView): HTMLElement {
  const { name, purpose } = entrance(view.endpoint);
  b.group = name;
  const titleId = `cn-sec-upmix-${view.endpoint}-title`;
  const state = upmixState(b.ctx.snapshot, view, true);
  const element = section(
    b,
    `upmix-${view.endpoint}`,
    name,
    [
      h('p', { class: 'cn-hint cn-section-sub' }, purpose),
      h(
        'div',
        { class: 'cn-rows' },
        row(text.status.stateLabel, [badge(state.tone, state.word)]),
        choices(b, view, titleId),
        view.effective === 'plugin' ? plugin(b, view) : null,
      ),
    ],
    { mark: laneMark(view.endpoint, 'cn-section-mark') },
  );
  b.group = null;
  return element;
}

export function upmixPage(b: Build): Page {
  const { snapshot } = b.ctx;
  const u = text.upmix;
  b.group = u.title;
  const apply = [
    upmixApply(b, 'headphones', text.profile.kindNames.headphones),
    upmixApply(b, 'speakers', text.profile.kindNames.speakers),
  ].filter((element): element is HTMLElement => element !== null);
  b.group = null;
  const element = h(
    'div',
    { class: 'cn-page cn-upmix' },
    section(b, 'upmix-all', text.console.upmixAll, [
      h(
        'p',
        { class: 'cn-text cn-measure cn-intro' },
        u.intro,
        ' ',
        infoTip('upmix-info', u.infoLabel, u.infoTip),
      ),
      summary(b),
      apply.length > 0 ? h('div', { class: 'cn-rows' }, ...apply) : null,
    ]),
    ...snapshot.upmix.endpoints.map((view) => endpointSection(b, view)),
  );
  return { element, fill: () => undefined };
}
