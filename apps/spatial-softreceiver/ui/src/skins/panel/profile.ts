// The profile page in the display, as a receiver's setup menu: the two
// output kinds as tabs (headphones with their BRIR set, speakers with a
// layout or a SECS 2R correction), the gain the encoder turns, a test signal
// per direction, and the sound position map. Every value comes from
// `Snapshot.profile`; which tab is open is view state, and Rust decides
// whether a change is taken (ADR 0034).

import type { EngineStatus, ProfileKind, SpeakerLayout } from '../../bindings';
import { type Child, h, liveText } from '../../dom';
import { SPEAKER_LAYOUTS } from '../../guard';
import { text } from '../../text';
import { type Context, type Drawn, described } from '../../screens/common';
import { GAIN_MAX, GAIN_MIN } from '../../screens/profile';
import { soundMap } from './map';
import { type Scope, signed } from './scope';
import { choices, dot, facts, given, hint, intentRow, line, pickRow, section, toggle } from './ui';

const KINDS: readonly ProfileKind[] = ['headphones', 'speakers'];
const LAYOUTS = Object.keys(SPEAKER_LAYOUTS) as SpeakerLayout[];
const FILE_HINT = 'profile-file-hint';
const RUNNING_HINT = 'profile-running-hint';

/** The ids that describe a change: the file that locks it, a change that takes seconds while running. */
function changeHints(ctx: Context): (string | false)[] {
  const { profile, phase } = ctx.snapshot;
  return [profile.from_file && FILE_HINT, !profile.from_file && phase.kind === 'running' && RUNNING_HINT];
}

/** profile.toml decides the output: its name and the sentence. */
function fileNote(ctx: Context): HTMLElement | null {
  const { profile } = ctx.snapshot;
  if (!profile.from_file) return null;
  return h(
    'div',
    { class: 'pn-note' },
    line(h('span', null, h('span', { class: 'pn-tag' }, text.profile.fromFileTag), ' ', profile.name)),
    hint(text.profile.fromFile, FILE_HINT),
  );
}

/** The two kinds as tabs; the kind in use says so. Opening one sends nothing. */
function tabs(ctx: Context, open: ProfileKind): HTMLElement {
  const inUse = ctx.snapshot.profile.kind;
  const choose = (kind: ProfileKind, focus: boolean) => {
    if (focus) list.querySelector<HTMLElement>(`[data-key="profile-tab-${kind}"]`)?.focus();
    ctx.setView({ profileTab: kind });
  };
  const list = h('div', {
    class: 'pn-tabs',
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
      choose(kind, true);
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
          class: 'pn-tab',
          'data-key': `profile-tab-${kind}`,
          'aria-selected': selected ? 'true' : 'false',
          'aria-controls': selected ? `profile-panel-${kind}` : null,
          tabindex: selected ? 0 : -1,
          onClick: () => choose(kind, false),
        },
        h('span', null, text.profile.kindNames[kind]),
        kind === inUse ? h('span', { class: 'pn-tag' }, text.profile.inUse) : null,
      ),
    );
  }
  return list;
}

/** Whether this kind applies the upmixers (ADR 0035), while some input's upmixer is on. */
export function upmixApply(ctx: Context, kind: ProfileKind, label: string): HTMLElement | null {
  const { upmix } = ctx.snapshot;
  if (upmix.endpoints.every((endpoint) => endpoint.effective === 'off')) return null;
  ctx.offer('set_upmix_apply');
  const sending = ctx.sending?.kind === 'set_upmix_apply' && ctx.sending.value.kind === kind ? ctx.sending.value : null;
  return toggle({
    key: `upmix-apply-${kind}`,
    label,
    on: sending?.on ?? upmix[kind],
    disabled: ctx.busy,
    busy: sending !== null,
    describedBy: 'upmix-apply-hint',
    toggle: (on) => ctx.send({ kind: 'set_upmix_apply', value: { kind, on } }),
  });
}

/** The end of a tab: the kind in use says so; the other offers the switch. */
function footer(ctx: Context, kind: ProfileKind): Child[] {
  const { profile } = ctx.snapshot;
  if (profile.kind === kind) {
    return [line(h('span', null, dot('white'), kind === 'headphones' ? text.profile.usingHeadphones : text.profile.usingSpeakers), 'pn-in-use')];
  }
  const needsBrir = kind === 'headphones' && profile.headphones.brir === null && !profile.from_file;
  return [
    intentRow(
      ctx,
      kind === 'headphones' ? text.profile.useHeadphones : text.profile.useSpeakers,
      { kind: 'set_profile_kind', value: kind },
      {
        key: `use-${kind}`,
        busyLabel: text.profile.switching,
        disabled: profile.from_file || needsBrir,
        describedBy: described(needsBrir && 'use-headphones-hint', ...changeHints(ctx)),
      },
    ),
    needsBrir ? hint(text.profile.needsBrir, 'use-headphones-hint') : null,
  ];
}

function applyRow(ctx: Context, kind: ProfileKind): Child[] {
  const apply = upmixApply(ctx, kind, text.profile.upmixApply);
  return apply === null ? [] : [apply, hint(text.profile.upmixApplyHint, 'upmix-apply-hint')];
}

function headphones(ctx: Context): Child[] {
  const { profile } = ctx.snapshot;
  const locked = profile.from_file;
  const importing = ctx.pending === 'import_brir';
  const ids = described(importing && 'brir-hint', ...changeHints(ctx));
  const send = (value: string) => ctx.send({ kind: 'import_brir', value });
  ctx.offer('import_brir');
  return [
    facts([[text.profile.brir, given(profile.headphones.brir)]]),
    hint(text.profile.brirBody),
    pickRow(ctx, text.profile.brirFolder, 'brir-folder', locked, ids, () => ctx.shell.pickFolder(text.profile.brirFolderDialog), send),
    pickRow(
      ctx,
      text.profile.brirFile,
      'brir-file',
      locked,
      ids,
      () => ctx.shell.pickFile(text.profile.brirFileDialog, { name: text.profile.brirFilter, extensions: ['wav'] }),
      send,
    ),
    importing ? hint(text.profile.importing, 'brir-hint') : null,
    ...applyRow(ctx, 'headphones'),
    ...footer(ctx, 'headphones'),
  ];
}

/** The layout the speakers use while no correction is imported. */
function layout(ctx: Context): HTMLElement {
  const { profile } = ctx.snapshot;
  const { layout: current, correction } = profile.speakers;
  const changing = ctx.pending === 'set_speaker_layout';
  const byCorrection = correction !== null && !profile.from_file;
  const note = changing ? text.profile.applying : byCorrection ? text.profile.layoutByCorrection : null;
  const children: Child[] = [];
  if (current === null) {
    // A speaker profile.toml lays the speakers out its own way.
    children.push(line(text.profile.layoutFromFile));
  } else {
    ctx.offer('set_speaker_layout');
    const sending = ctx.sending?.kind === 'set_speaker_layout' ? ctx.sending.value : null;
    children.push(
      choices({
        labelledBy: 'pn-layout-title',
        describedBy: described(note !== null && 'layout-hint', ...changeHints(ctx)),
        options: LAYOUTS.map((value) => ({ value, label: value, key: `speaker-layout-${value}` })),
        chosen: sending ?? current,
        disabled: ctx.busy || profile.from_file || correction !== null,
        busy: changing,
        intent: 'set_speaker_layout',
        inline: true,
        choose: (value) => ctx.send({ kind: 'set_speaker_layout', value }),
      }),
    );
  }
  if (note !== null) children.push(hint(note, 'layout-hint'));
  return section('pn-layout', text.profile.layout, children, 'pn-sub');
}

/** The SECS 2R correction: the file, its title, and the rows that set and clear it. */
function correction(ctx: Context): HTMLElement {
  const { profile } = ctx.snapshot;
  const current = profile.speakers.correction;
  const locked = profile.from_file;
  const note =
    ctx.pending === 'import_correction' ? text.profile.importing
    : ctx.pending === 'clear_correction' ? text.profile.applying
    : null;
  const ids = described(note !== null && 'correction-hint', ...changeHints(ctx));
  const rows: [string, Child][] = [[text.profile.correctionFile, given(current?.path ?? null)]];
  if (current !== null) rows.push([text.profile.correctionName, current.title]);
  ctx.offer('import_correction');
  return section(
    'pn-correction',
    text.profile.correctionTitle,
    [
      hint(text.profile.correctionBody),
      facts(rows),
      pickRow(
        ctx,
        text.profile.correctionImport,
        'correction-import',
        locked,
        ids,
        () => ctx.shell.pickFile(text.profile.correctionDialog, { name: text.profile.correctionFilter, extensions: ['yml', 'yaml'] }),
        (value) => ctx.send({ kind: 'import_correction', value }),
      ),
      current === null
        ? null
        : intentRow(ctx, text.profile.correctionClear, { kind: 'clear_correction' }, { key: 'correction-clear', disabled: locked, describedBy: ids }),
      note === null ? null : hint(note, 'correction-hint'),
    ],
    'pn-sub',
  );
}

/** What each device channel plays. */
function channels(ctx: Context): HTMLElement {
  const rows = ctx.snapshot.profile.speakers.channels.map((channel) =>
    h(
      'tr',
      null,
      h('td', { class: 'num' }, channel.channel + 1),
      h('th', { scope: 'row' }, channel.name, channel.subwoofer ? h('span', { class: 'pn-tag' }, text.profile.subwoofer) : null),
    ),
  );
  return section(
    'pn-channels',
    text.profile.channelsTitle,
    [
      h(
        'table',
        { class: 'pn-table', 'aria-labelledby': 'pn-channels-title' },
        h('thead', null, h('tr', null, h('th', { scope: 'col' }, text.profile.channel), h('th', { scope: 'col' }, text.profile.speaker))),
        h('tbody', null, ...rows),
      ),
    ],
    'pn-sub',
  );
}

function speakers(ctx: Context): Child[] {
  return [layout(ctx), correction(ctx), channels(ctx), ...applyRow(ctx, 'speakers'), ...footer(ctx, 'speakers')];
}

/** The gain the encoder turns: the numeral follows it, the range under it. */
function gain(scope: Scope): HTMLElement {
  const value = liveText(signed(scope.gain.value()));
  scope.gain.follow((db) => value.set(signed(db)));
  return section('pn-gain', text.profile.gainTitle, [
    h('p', { class: 'pn-big' }, h('span', { class: 'num' }, value.node), h('small', null, text.profile.gainUnit)),
    h('p', { class: 'pn-scale num', 'aria-hidden': 'true' }, `${signed(GAIN_MIN)} … ${signed(GAIN_MAX)} ${text.profile.gainUnit}`),
    hint(text.panel.gainBody),
  ]);
}

/** A test signal per direction of the layout in use; only while running. */
function tests(ctx: Context): HTMLElement {
  const { profile, phase } = ctx.snapshot;
  const running = phase.kind === 'running';
  const empty = profile.directions.length === 0;
  const list = h(
    'div',
    { class: 'pn-tests', role: 'group', 'aria-label': text.profile.testLabel },
    ...profile.directions.map((direction) =>
      intentRow(
        ctx,
        direction.name,
        { kind: 'test_direction', value: direction.id },
        {
          key: `direction-${direction.id}`,
          detail: text.profile.angles(direction.azimuth_deg, direction.elevation_deg),
          disabled: !running,
          describedBy: running ? null : 'test-hint',
        },
      ),
    ),
  );
  return section('pn-tests', text.profile.testTitle, [
    hint(text.profile.testBody),
    empty ? line(text.profile.noDirections) : list,
    running || empty ? null : hint(text.profile.testOff, 'test-hint'),
  ]);
}

export function profilePage(scope: Scope, engineNow: EngineStatus | null): Drawn {
  const { ctx } = scope;
  const { snapshot } = ctx;
  const running = snapshot.phase.kind === 'running';
  const open = ctx.view.profileTab ?? snapshot.profile.kind;
  const engine = running ? engineNow : null;
  const map = soundMap(ctx, engine?.directions ?? snapshot.profile.directions, running, 'map-direction');
  if (engine !== null) map.update(engine);

  const element = h(
    'div',
    { class: 'pn-profile' },
    h(
      'div',
      { class: 'pn-profile-main' },
      fileNote(ctx),
      section('pn-kinds', text.profile.outputTitle, [
        tabs(ctx, open),
        h(
          'div',
          {
            role: 'tabpanel',
            id: `profile-panel-${open}`,
            class: 'pn-tabpanel',
            'data-region': `profile-${open}`,
            'aria-labelledby': `profile-tab-${open}`,
          },
          ...(open === 'headphones' ? headphones(ctx) : speakers(ctx)),
        ),
        running && !snapshot.profile.from_file ? hint(text.profile.importRunning, RUNNING_HINT) : null,
      ]),
    ),
    h(
      'div',
      { class: 'pn-profile-side' },
      gain(scope),
      tests(ctx),
      section('pn-profile-map', text.profile.mapTitle, [hint(text.profile.mapBody), map.element]),
    ),
  );
  return running ? { element, live: { update: map.update } } : { element };
}
