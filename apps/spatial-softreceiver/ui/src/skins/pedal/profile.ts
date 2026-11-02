// The profile face (ADR 0046, ADR 0034). The output kind in use is the
// slide switch (an intent); which kind's settings stand on the face is view
// state, two illuminated keys whose ring is lit on the kind shown.
// Headphones: the BRIR set printed and the momentary keys that pick its
// folder or file. Speakers: the sixteen layouts as illuminated keys in four
// family lines of one radio group, the SECS 2R correction's keys and facts,
// and the channel table printed. Then the LEVEL encoder, each kind's upmix
// switch, a test signal per direction (the map's buttons and a key per
// direction listed under it), and the sound map.

import type { EngineStatus, ProfileKind, SpeakerLayout } from '../../bindings';
import { type Child, h } from '../../dom';
import { SPEAKER_LAYOUTS } from '../../guard';
import { text } from '../../text';
import { type Context, type Drawn, described } from '../../screens/common';
import { GAIN_MAX, GAIN_MIN } from '../../screens/profile';
import { kindSlide, layoutFamilies, levelEncoder, upmixApply } from './controls';
import { DENSE_DIRECTIONS, soundMap } from './map';
import { facts, fine, group, ikey, intentKey, keyRow, mkey, pickKey, print } from './parts';
import { type Scope, signed } from './scope';

const KINDS: readonly ProfileKind[] = ['headphones', 'speakers'];
const LAYOUTS = Object.keys(SPEAKER_LAYOUTS) as SpeakerLayout[];
/** The sixteen layouts one family a line. */
const LAYOUT_LINES = layoutFamilies(LAYOUTS);
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
    { class: 'pd-filenote' },
    h('p', { class: 'pd-small' }, print(text.profile.fromFileTag, 'pd-tag'), ' ', profile.name),
    fine(text.profile.fromFile, FILE_HINT),
  );
}

/** The two keys that choose which kind's settings stand on the face: tabs, the ring lit on the one shown. */
function kindKeys(ctx: Context, open: ProfileKind): HTMLElement {
  const inUse = ctx.snapshot.profile.kind;
  const choose = (kind: ProfileKind, focus: boolean) => {
    if (focus) list.querySelector<HTMLElement>(`[data-key="profile-tab-${kind}"]`)?.focus();
    ctx.setView({ profileTab: kind });
  };
  const list = h('div', {
    class: 'pd-kindkeys',
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
      ikey({
        key: `profile-tab-${kind}`,
        label: text.profile.kindNames[kind],
        detail: kind === inUse ? text.profile.inUse : undefined,
        lit: selected,
        chosen: selected,
        attrs: {
          role: 'tab',
          id: `profile-tab-${kind}`,
          'aria-selected': selected ? 'true' : 'false',
          'aria-controls': `profile-panel-${kind}`,
          tabindex: selected ? 0 : -1,
        },
        onClick: () => choose(kind, false),
      }),
    );
  }
  return list;
}

function applyLine(ctx: Context, kind: ProfileKind): Child[] {
  const apply = upmixApply(ctx, kind, text.profile.upmixApply);
  return apply === null ? [] : [apply, fine(text.profile.upmixApplyHint, 'upmix-apply-hint')];
}

function headphones(ctx: Context): Child[] {
  const { profile } = ctx.snapshot;
  const locked = profile.from_file;
  const importing = ctx.pending === 'import_brir';
  const ids = described(importing && 'brir-hint', ...changeHints(ctx));
  const send = (value: string) => ctx.send({ kind: 'import_brir', value });
  ctx.offer('import_brir');
  return [
    facts([[text.profile.brir, h('span', { class: profile.headphones.brir === null ? 'pd-none' : 'pd-path' }, profile.headphones.brir ?? text.profile.none)]]),
    fine(text.profile.brirBody),
    h(
      'div',
      { class: 'pd-keys' },
      pickKey(ctx, text.profile.brirFolder, 'brir-folder', locked, ids, () => ctx.shell.pickFolder(text.profile.brirFolderDialog), send),
      pickKey(
        ctx,
        text.profile.brirFile,
        'brir-file',
        locked,
        ids,
        () => ctx.shell.pickFile(text.profile.brirFileDialog, { name: text.profile.brirFilter, extensions: ['wav'] }),
        send,
      ),
    ),
    importing ? fine(text.profile.importing, 'brir-hint') : null,
    ...applyLine(ctx, 'headphones'),
  ];
}

/** The layout the speakers use while no correction is imported. */
function layout(ctx: Context): HTMLElement {
  const { profile } = ctx.snapshot;
  const { layout: current, correction } = profile.speakers;
  const changing = ctx.pending === 'set_speaker_layout';
  const byCorrection = correction !== null && !profile.from_file;
  const hint = changing ? text.profile.applying : byCorrection ? text.profile.layoutByCorrection : null;
  const children: Child[] = [];
  if (current === null) {
    // A speaker profile.toml lays the speakers out its own way.
    children.push(fine(text.profile.layoutFromFile));
  } else {
    ctx.offer('set_speaker_layout');
    const sending = ctx.sending?.kind === 'set_speaker_layout' ? ctx.sending.value : null;
    children.push(
      keyRow({
        labelledBy: 'pd-layout-title',
        describedBy: described(hint !== null && 'layout-hint', ...changeHints(ctx)),
        choices: LAYOUTS.map((value) => ({ value, label: value, key: `speaker-layout-${value}` })),
        chosen: sending ?? current,
        off: profile.from_file || correction !== null,
        locked: ctx.busy,
        busy: changing,
        intent: 'set_speaker_layout',
        lines: LAYOUT_LINES,
        size: 'sm',
        choose: (value) => ctx.send({ kind: 'set_speaker_layout', value }),
      }),
    );
  }
  if (hint !== null) children.push(fine(hint, 'layout-hint'));
  return group('pd-layout', text.profile.layout, children, 'pd-sub');
}

/** The SECS 2R correction: the file, its title, and the keys that set and clear it. */
function correction(ctx: Context): HTMLElement {
  const { profile } = ctx.snapshot;
  const current = profile.speakers.correction;
  const locked = profile.from_file;
  const hint =
    ctx.pending === 'import_correction' ? text.profile.importing
    : ctx.pending === 'clear_correction' ? text.profile.applying
    : null;
  const ids = described(hint !== null && 'correction-hint', ...changeHints(ctx));
  const rows: [string, Child][] = [
    [text.profile.correctionFile, h('span', { class: current === null ? 'pd-none' : 'pd-path' }, current?.path ?? text.profile.none)],
  ];
  if (current !== null) rows.push([text.profile.correctionName, current.title]);
  ctx.offer('import_correction');
  return group(
    'pd-correction',
    text.profile.correctionTitle,
    [
      fine(text.profile.correctionBody),
      facts(rows),
      h(
        'div',
        { class: 'pd-keys' },
        pickKey(
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
          : intentKey(ctx, text.profile.correctionClear, { kind: 'clear_correction' }, { key: 'correction-clear', off: locked, describedBy: ids }),
      ),
      hint === null ? null : fine(hint, 'correction-hint'),
    ],
    'pd-sub',
  );
}

/** What each device channel plays, printed. */
function channels(ctx: Context): HTMLElement {
  const rows = ctx.snapshot.profile.speakers.channels.map((channel) =>
    h(
      'tr',
      null,
      h('td', { class: 'pd-num' }, channel.channel + 1),
      h('th', { scope: 'row' }, channel.name, channel.subwoofer ? h('span', { class: 'pd-tag' }, text.profile.subwoofer) : null),
    ),
  );
  return group(
    'pd-channels',
    text.profile.channelsTitle,
    [
      h(
        'table',
        { class: 'pd-table', 'aria-labelledby': 'pd-channels-title' },
        h('thead', null, h('tr', null, h('th', { scope: 'col' }, text.profile.channel), h('th', { scope: 'col' }, text.profile.speaker))),
        h('tbody', null, ...rows),
      ),
    ],
    'pd-sub',
  );
}

function speakers(ctx: Context): Child[] {
  return [layout(ctx), correction(ctx), channels(ctx), ...applyLine(ctx, 'speakers')];
}

/** The LEVEL encoder with the range printed by it. */
function gain(scope: Scope): HTMLElement {
  return group('pd-gain', text.profile.gainTitle, [
    levelEncoder(scope, 64),
    print(`${signed(GAIN_MIN)} … ${signed(GAIN_MAX)} ${text.profile.gainUnit}`, 'pd-small pd-num'),
    fine(text.pedal.gainBody),
  ]);
}

/**
 * A momentary key per direction of the layout in use; only while running.
 * A layout of many directions (22.2, 24.1.10, 30.2) lists them in narrower
 * columns with the angles as bare numbers; each key still says its angles
 * in words to a screen reader.
 */
function tests(ctx: Context): HTMLElement {
  const { profile, phase } = ctx.snapshot;
  const running = phase.kind === 'running';
  const empty = profile.directions.length === 0;
  const dense = profile.directions.length > DENSE_DIRECTIONS;
  if (running) ctx.offer('test_direction');
  const list = h(
    'div',
    { class: `pd-tests${dense ? ' is-dense' : ''}`, role: 'group', 'aria-label': text.profile.testLabel },
    ...profile.directions.map((direction) => {
      const angles = text.profile.angles(direction.azimuth_deg, direction.elevation_deg);
      const key = mkey({
        key: `direction-${direction.id}`,
        label: direction.name,
        detail: dense ? `${Math.round(direction.azimuth_deg)}° / ${Math.round(direction.elevation_deg)}°` : angles,
        off: !running,
        locked: ctx.busy,
        busy: ctx.sending?.kind === 'test_direction' && ctx.sending.value === direction.id,
        intent: 'test_direction',
        describedBy: running ? null : 'test-hint',
        onClick: () => ctx.send({ kind: 'test_direction', value: direction.id }),
      });
      if (dense) {
        key.setAttribute('aria-label', `${direction.name}, ${angles}`);
        key.title = angles;
      }
      return key;
    }),
  );
  return group('pd-tests', text.profile.testTitle, [
    fine(text.profile.testBody),
    empty ? fine(text.profile.noDirections) : list,
    running || empty ? null : fine(text.profile.testOff, 'test-hint'),
  ]);
}

export function profileFace(scope: Scope, engineNow: EngineStatus | null): Drawn {
  const { ctx } = scope;
  const { snapshot } = ctx;
  const running = snapshot.phase.kind === 'running';
  const open = ctx.view.profileTab ?? snapshot.profile.kind;
  const engine = running ? engineNow : null;
  const map = soundMap(ctx, engine?.directions ?? snapshot.profile.directions, running, 'map-direction');
  if (engine !== null) map.update(engine);

  const element = h(
    'div',
    { class: 'pd-profile' },
    h(
      'div',
      { class: 'pd-profile-main' },
      fileNote(ctx),
      group('pd-kinds', text.profile.outputTitle, [
        kindSlide(scope),
        kindKeys(ctx, open),
        h(
          'div',
          {
            role: 'tabpanel',
            id: `profile-panel-${open}`,
            class: 'pd-kindpanel',
            'data-region': `profile-${open}`,
            'aria-labelledby': `profile-tab-${open}`,
          },
          ...(open === 'headphones' ? headphones(ctx) : speakers(ctx)),
        ),
        running && !snapshot.profile.from_file ? fine(text.profile.importRunning, RUNNING_HINT) : null,
      ]),
    ),
    h(
      'div',
      { class: 'pd-profile-side' },
      gain(scope),
      group('pd-profile-map', text.profile.mapTitle, [fine(text.pedal.mapBody), map.element, running ? fine(text.pedal.mapTest) : null]),
      tests(ctx),
    ),
  );
  return running ? { element, live: { update: map.update } } : { element };
}
