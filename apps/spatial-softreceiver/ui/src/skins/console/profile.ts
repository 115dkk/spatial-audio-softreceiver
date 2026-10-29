// The profile page: the output kinds as tabs (headphones with their BRIR set,
// speakers with a layout or a SECS 2R correction and the device channels as a
// table), the gain, a test tone per direction and the sound positions. The
// same capabilities as the standard profile screen, as label | control rows.

import type { EngineStatus, ProfileKind, SpeakerLayout } from '../../bindings';
import { type Child, h } from '../../dom';
import { oneDecimal } from '../../format';
import { SPEAKER_LAYOUTS } from '../../guard';
import { described } from '../../screens/common';
import { GAIN_MAX, GAIN_MIN } from '../../screens/profile';
import { text } from '../../text';
import {
  type Build,
  act,
  badge,
  choose,
  hint,
  para,
  pick,
  range,
  row,
  section,
  table,
  toggle,
} from './kit';
import { sceneMap } from './map';
import { laneTitle, lanesOf, type Page } from './overview';

const KINDS: readonly ProfileKind[] = ['headphones', 'speakers'];
const LAYOUTS = Object.keys(SPEAKER_LAYOUTS) as SpeakerLayout[];
const FILE_HINT = 'profile-file-hint';
const RUNNING_HINT = 'profile-running-hint';

const path = (value: string | null) => h('span', { class: value === null ? 'cn-muted' : 'cn-path' }, value ?? text.profile.none);

/** profile.toml locks every change here; a running engine takes a few seconds for one. */
function changeHints(b: Build): (string | false)[] {
  const { profile, phase } = b.ctx.snapshot;
  return [profile.from_file && FILE_HINT, !profile.from_file && phase.kind === 'running' && RUNNING_HINT];
}

const fileLock = (b: Build): string | null => (b.ctx.snapshot.profile.from_file ? text.profile.fromFile : null);

function tabs(b: Build, open: ProfileKind): HTMLElement {
  const { ctx } = b;
  const inUse = ctx.snapshot.profile.kind;
  const pickTab = (kind: ProfileKind, focus: boolean) => {
    if (focus) list.querySelector<HTMLElement>(`[data-key="profile-tab-${kind}"]`)?.focus();
    ctx.setView({ profileTab: kind });
  };
  const list = h('div', {
    class: 'cn-tabs',
    role: 'tablist',
    'aria-label': text.profile.outputTitle,
    onKeydown: (event: Event) => {
      const key = (event as KeyboardEvent).key;
      const at = KINDS.indexOf(open);
      const to =
        key === 'ArrowRight' ? (at + 1) % KINDS.length
        : key === 'ArrowLeft' ? (at - 1 + KINDS.length) % KINDS.length
        : key === 'Home' ? 0
        : key === 'End' ? KINDS.length - 1
        : null;
      const kind = to === null ? undefined : KINDS[to];
      if (kind === undefined) return;
      event.preventDefault();
      pickTab(kind, true);
    },
  });
  for (const kind of KINDS) {
    const selected = kind === open;
    list.append(
      h(
        'button',
        {
          type: 'button',
          role: 'tab',
          id: `profile-tab-${kind}`,
          class: 'cn-tab',
          'data-key': `profile-tab-${kind}`,
          'aria-selected': selected ? 'true' : 'false',
          'aria-controls': selected ? `profile-panel-${kind}` : null,
          tabindex: selected ? 0 : -1,
          onClick: () => pickTab(kind, false),
        },
        h('span', null, text.profile.kindNames[kind]),
        kind === inUse ? h('span', { class: 'cn-tab-tag' }, text.profile.inUse) : null,
      ),
    );
  }
  return list;
}

/** The kind in use says so; the other offers the switch to it. */
function footer(b: Build, kind: ProfileKind): HTMLElement {
  const { profile, phase } = b.ctx.snapshot;
  const p = text.profile;
  const children: Child[] = [];
  if (profile.kind === kind) {
    children.push(badge('good', kind === 'headphones' ? p.usingHeadphones : p.usingSpeakers));
  } else {
    const needsBrir = kind === 'headphones' && profile.headphones.brir === null && !profile.from_file;
    children.push(
      act(
        b,
        kind === 'headphones' ? p.useHeadphones : p.useSpeakers,
        { kind: 'set_profile_kind', value: kind },
        {
          key: `use-${kind}`,
          variant: 'ink',
          pendingLabel: p.switching,
          locked: fileLock(b) ?? (needsBrir ? p.needsBrir : null),
          describedBy: described(needsBrir && 'use-headphones-hint', ...changeHints(b)),
        },
      ),
    );
    if (needsBrir) children.push(hint(p.needsBrir, 'use-headphones-hint'));
  }
  if (phase.kind === 'running' && !profile.from_file) children.push(hint(p.importRunning, RUNNING_HINT));
  return h('div', { class: 'cn-tab-footer' }, ...children);
}

/** Whether this kind applies the upmixers; shown while some endpoint's upmixer is not off. */
export function upmixApply(b: Build, kind: ProfileKind, label: string): HTMLElement | null {
  const { ctx } = b;
  const { upmix } = ctx.snapshot;
  if (upmix.endpoints.every((endpoint) => endpoint.effective === 'off')) return null;
  const sending = ctx.sending?.kind === 'set_upmix_apply' && ctx.sending.value.kind === kind ? ctx.sending.value : null;
  const hintId = `upmix-apply-${kind}-hint`;
  const saved = b.group;
  b.group = text.profile.kindNames[kind];
  const control = toggle(b, {
      key: `upmix-apply-${kind}`,
      label: text.profile.upmixApply,
      on: sending?.on ?? upmix[kind],
      busy: sending !== null,
      describedBy: hintId,
      onToggle: (on) => ctx.send({ kind: 'set_upmix_apply', value: { kind, on } }),
  });
  b.group = saved;
  return row(label, [control, hint(text.profile.upmixApplyHint, hintId)]);
}

function headphones(b: Build): Child[] {
  const { ctx } = b;
  const { profile } = ctx.snapshot;
  const p = text.profile;
  const importing = ctx.pending === 'import_brir';
  const ids = described(importing && 'brir-hint', ...changeHints(b));
  const send = (value: string) => ctx.send({ kind: 'import_brir', value });
  return [
    row(p.brir, [
      path(profile.headphones.brir),
      para(p.brirBody, 'cn-measure'),
      h(
        'div',
        { class: 'cn-actions' },
        pick(b, p.brirFolder, 'brir-folder', fileLock(b), ids, () => ctx.shell.pickFolder(p.brirFolderDialog), send),
        pick(
          b,
          p.brirFile,
          'brir-file',
          fileLock(b),
          ids,
          () => ctx.shell.pickFile(p.brirFileDialog, { name: p.brirFilter, extensions: ['wav'] }),
          send,
        ),
      ),
      importing ? hint(p.importing, 'brir-hint') : null,
    ]),
    upmixApply(b, 'headphones', p.upmixApply),
    footer(b, 'headphones'),
  ];
}

function layoutRow(b: Build): HTMLElement {
  const { ctx } = b;
  const { profile } = ctx.snapshot;
  const { layout, correction } = profile.speakers;
  const p = text.profile;
  const changing = ctx.pending === 'set_speaker_layout';
  const byCorrection = correction !== null && !profile.from_file;
  const note = changing ? p.applying : byCorrection ? p.layoutByCorrection : null;
  const select = choose<SpeakerLayout>(b, {
    id: 'speaker-layout',
    key: 'speaker-layout',
    label: p.layout,
    options: LAYOUTS.map((value) => [value, value] as const),
    current: layout,
    placeholder: p.layoutFromFile,
    locked: fileLock(b) ?? (layout === null ? p.layoutFromFile : correction !== null ? p.layoutByCorrection : null),
    busy: changing,
    describedBy: described(note !== null && 'layout-hint', ...changeHints(b)),
    onChoose: (value) => ctx.send({ kind: 'set_speaker_layout', value }),
  });
  return row(p.layout, [select, note === null ? null : hint(note, 'layout-hint')], { for: 'speaker-layout' });
}

function correctionRow(b: Build): HTMLElement {
  const { ctx } = b;
  const { profile } = ctx.snapshot;
  const correction = profile.speakers.correction;
  const p = text.profile;
  const note = ctx.pending === 'import_correction' ? p.importing : ctx.pending === 'clear_correction' ? p.applying : null;
  const ids = described(note !== null && 'correction-hint', ...changeHints(b));
  return row(p.correctionTitle, [
    h(
      'dl',
      { class: 'cn-facts' },
      h('div', null, h('dt', null, p.correctionFile), h('dd', null, path(correction?.path ?? null))),
      correction === null ? null : h('div', null, h('dt', null, p.correctionName), h('dd', null, correction.title)),
    ),
    para(p.correctionBody, 'cn-measure'),
    h(
      'div',
      { class: 'cn-actions' },
      pick(
        b,
        p.correctionImport,
        'correction-import',
        fileLock(b),
        ids,
        () => ctx.shell.pickFile(p.correctionDialog, { name: p.correctionFilter, extensions: ['yml', 'yaml'] }),
        (value) => ctx.send({ kind: 'import_correction', value }),
      ),
      correction === null
        ? null
        : act(b, p.correctionClear, { kind: 'clear_correction' }, { key: 'correction-clear', locked: fileLock(b), describedBy: ids }),
    ),
    note === null ? null : hint(note, 'correction-hint'),
  ]);
}

function channels(b: Build): HTMLElement {
  const p = text.profile;
  const rows = b.ctx.snapshot.profile.speakers.channels.map((channel) =>
    h(
      'tr',
      null,
      h('td', { class: 'cn-num cn-cell-num' }, channel.channel + 1),
      h(
        'th',
        { scope: 'row' },
        channel.name,
        channel.subwoofer ? h('span', { class: 'cn-tag' }, p.subwoofer) : null,
      ),
    ),
  );
  return row(h('span', { id: 'channels-title' }, p.channelsTitle), [
    table({ labelledBy: 'channels-title', head: [p.channel, p.speaker], rows, className: 'cn-channel-table' }),
  ]);
}

function speakers(b: Build): Child[] {
  return [layoutRow(b), correctionRow(b), channels(b), upmixApply(b, 'speakers', text.profile.upmixApply), footer(b, 'speakers')];
}

function output(b: Build): HTMLElement {
  const { ctx } = b;
  const { profile } = ctx.snapshot;
  const open = ctx.view.profileTab ?? profile.kind;
  b.group = text.profile.kindNames[open];
  const note = profile.from_file
    ? h(
        'div',
        { class: 'cn-note' },
        h('span', { class: 'cn-tag' }, text.profile.fromFileTag),
        h('span', { class: 'cn-value' }, profile.name),
        h('p', { id: FILE_HINT, class: 'cn-hint' }, text.profile.fromFile),
      )
    : null;
  const element = section(b, 'kind', text.profile.outputTitle, [
    note,
    tabs(b, open),
    h(
      'div',
      {
        role: 'tabpanel',
        id: `profile-panel-${open}`,
        'data-region': `profile-${open}`,
        class: 'cn-tabpanel cn-rows',
        'aria-labelledby': `profile-tab-${open}`,
        tabindex: 0,
      },
      ...(open === 'headphones' ? headphones(b) : speakers(b)),
    ),
  ]);
  b.group = null;
  return element;
}

function gain(b: Build): HTMLElement {
  const { ctx } = b;
  const p = text.profile;
  const value = ctx.pending === 'set_gain' && ctx.view.gainSending !== null ? ctx.view.gainSending : ctx.snapshot.profile.gain_db;
  const say = (db: number) => `${oneDecimal(db)} ${p.gainUnit}`;
  return section(b, 'gain', p.gainTitle, [
    row(p.gainLabel, [
      range(b, {
        id: 'gain',
        label: p.gainLabel,
        min: GAIN_MIN,
        max: GAIN_MAX,
        step: 0.5,
        value,
        say,
        minLabel: `${GAIN_MIN}`,
        maxLabel: `+${GAIN_MAX}`,
        describedBy: 'gain-hint',
        onCommit: (wanted) => {
          ctx.setView({ gainSending: wanted });
          ctx.send({ kind: 'set_gain', value: wanted });
        },
      }),
      hint(p.gainBody, 'gain-hint'),
    ], { for: 'gain' }),
  ]);
}

function tones(b: Build): HTMLElement {
  const { ctx } = b;
  const { profile, phase } = ctx.snapshot;
  const p = text.profile;
  const running = phase.kind === 'running';
  b.group = p.testTitle;
  const grid = h('div', { class: 'cn-directions', role: 'group', 'aria-label': p.testLabel });
  for (const direction of profile.directions) {
    const tone = act(
      b,
      direction.name,
      { kind: 'test_direction', value: direction.id },
      {
        key: `direction-${direction.id}`,
        locked: running ? null : p.testOff,
        describedBy: running ? null : 'test-hint',
        mine: ctx.sending?.kind === 'test_direction' && ctx.sending.value === direction.id,
      },
    );
    tone.title = p.angles(direction.azimuth_deg, direction.elevation_deg);
    tone.append(h('span', { class: 'cn-direction-angle cn-num', 'aria-hidden': 'true' }, `${Math.round(direction.azimuth_deg)}° / ${Math.round(direction.elevation_deg)}°`));
    grid.append(tone);
  }
  b.group = null;
  const empty = profile.directions.length === 0;
  return section(b, 'tones', p.testTitle, [
    para(p.testBody, 'cn-measure'),
    empty ? hint(p.noDirections) : grid,
    running || empty ? null : hint(p.testOff, 'test-hint'),
  ]);
}

export function profilePage(b: Build, engineNow: EngineStatus | null): Page {
  const { snapshot } = b.ctx;
  const running = snapshot.phase.kind === 'running';
  const engine = running ? engineNow : null;
  const map = sceneMap(engine?.directions ?? snapshot.profile.directions, engine !== null, lanesOf(snapshot), (lane) =>
    laneTitle(snapshot, lane),
  );
  if (engine !== null) map.update(engine);
  const element = h(
    'div',
    { class: 'cn-page cn-profile' },
    output(b),
    gain(b),
    tones(b),
    section(b, 'map', text.profile.mapTitle, [map.element], { className: 'cn-map-section' }),
  );
  return { element, fill: (next) => map.update(next) };
}
