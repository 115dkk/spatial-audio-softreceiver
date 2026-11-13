// The profile face (ADR 0046, ADR 0034). The output kind in use is the
// slide switch (an intent); which kind's settings stand on the face is view
// state, two illuminated keys whose ring is lit on the kind shown.
// Headphones: the BRIR set's two card slots, a folder's and a file's, the set
// in use standing as a card in one. Speakers: the sixteen layouts as
// illuminated keys in four family lines of one radio group, the SECS 2R
// correction's card slot with its eject key and the title on a spec plate,
// and the channel table printed as a patch strip with an LED per device
// channel, lit while the speakers play through it. Then the LEVEL encoder,
// each kind's upmix switch, a test signal per direction (the map's buttons
// and a key per direction listed under it), and the sound map. Facts stand
// on spec plates, and a group's sentences are parted from its parts by
// printed hairlines (법칙 5).

import type { EngineStatus, ProfileKind, SpeakerLayout } from '../../bindings';
import { type Child, h } from '../../dom';
import { SPEAKER_LAYOUTS } from '../../guard';
import { text } from '../../text';
import { type Context, type Drawn, described } from '../../screens/common';
import { GAIN_MAX, GAIN_MIN } from '../../screens/profile';
import { kindSlide, layoutFamilies, levelEncoder, upmixApply } from './controls';
import { DENSE_DIRECTIONS, soundMap } from './map';
import { type Led, baseName, fine, flowRoom, group, hairline, ikey, keyRow, led, manual, manualRule, mkey, print, slot, spec, spoken, sr } from './parts';
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
        // The kind in use is the slide switch's to show (법칙 3); a dark tab key prints no "in use" by it.
        label: kind === inUse ? spoken(text.profile.kindNames[kind], text.profile.inUse) : text.profile.kindNames[kind],
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
  return apply === null ? [] : [hairline(), apply, manual(text.profile.upmixApplyHint, 'upmix-apply-hint')];
}

/** A BRIR set read from one WAV file rather than a folder of them. */
const isWav = (path: string) => /\.wav$/i.test(path);

/**
 * The BRIR set as two card slots side by side, a folder's and a file's (법칙
 * 2): a set comes in either way, and each way has its own picker. The set
 * in use stands as a card in the slot of its kind, its name on the label;
 * putting a set in either slot replaces it. What a set may be is the slots'
 * description, for a screen reader.
 */
function headphones(ctx: Context): Child[] {
  const { profile } = ctx.snapshot;
  const locked = profile.from_file;
  const brir = profile.headphones.brir;
  const sending = ctx.sending?.kind === 'import_brir' ? ctx.sending.value : null;
  const ids = described('brir-about', sending !== null && 'brir-hint', ...changeHints(ctx));
  const send = (value: string) => ctx.send({ kind: 'import_brir', value });
  const card = brir === null ? null : { name: baseName(brir), path: brir };
  const asFile = brir !== null && isWav(brir);
  ctx.offer('import_brir');
  return [
    h(
      'div',
      { class: 'pd-slots' },
      slot(ctx, {
        key: 'brir-folder',
        label: text.profile.brirFolderDialog,
        card: asFile ? null : card,
        describedBy: ids,
        off: locked,
        busy: sending !== null && !isWav(sending),
        intent: 'import_brir',
        pick: () => ctx.shell.pickFolder(text.profile.brirFolderDialog),
        send,
      }),
      slot(ctx, {
        key: 'brir-file',
        label: text.profile.brirFileDialog,
        card: asFile ? card : null,
        describedBy: ids,
        off: locked,
        busy: sending !== null && isWav(sending),
        intent: 'import_brir',
        pick: () => ctx.shell.pickFile(text.profile.brirFileDialog, { name: text.profile.brirFilter, extensions: ['wav'] }),
        send,
      }),
    ),
    sr(text.profile.brirBody, 'brir-about'),
    sending === null ? null : sr(text.profile.importing, 'brir-hint'),
    ...applyLine(ctx, 'headphones'),
  ];
}

/** The layout the speakers use while no correction is imported. */
function layout(ctx: Context): HTMLElement {
  const { profile } = ctx.snapshot;
  const { layout: current, correction } = profile.speakers;
  const changing = ctx.pending === 'set_speaker_layout';
  const byCorrection = correction !== null && !profile.from_file;
  // The key being sent blinks (법칙 10); "applying" is only for a screen reader.
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
  if (changing) children.push(sr(text.profile.applying, 'layout-hint'));
  else if (hint !== null) children.push(hairline(), fine(hint, 'layout-hint'));
  return group('pd-layout', text.profile.layout, children, 'pd-sub');
}

/**
 * The SECS 2R correction as a card slot (법칙 2): the slot named for what
 * goes in it (a CamillaDSP configuration), the file in use standing out of
 * it as a card with the file's name on its label and a small eject key that
 * clears it, the correction's title on a spec plate under it. What to put in
 * is the slot's description, for a screen reader.
 */
function correction(ctx: Context): HTMLElement {
  const { profile } = ctx.snapshot;
  const current = profile.speakers.correction;
  const locked = profile.from_file;
  const importing = ctx.pending === 'import_correction';
  const clearing = ctx.pending === 'clear_correction';
  const hint = importing ? text.profile.importing : clearing ? text.profile.applying : null;
  const ids = described('correction-about', hint !== null && 'correction-hint', ...changeHints(ctx));
  ctx.offer('import_correction');
  let eject: HTMLElement | null = null;
  if (current !== null) {
    ctx.offer('clear_correction');
    eject = mkey({
      key: 'correction-clear',
      label: text.profile.correctionClear,
      size: 'sm',
      beside: true,
      off: locked,
      locked: ctx.busy,
      busy: clearing,
      intent: 'clear_correction',
      describedBy: described(hint !== null && 'correction-hint', ...changeHints(ctx)),
      onClick: () => ctx.send({ kind: 'clear_correction' }),
    });
    eject.classList.add('pd-eject');
  }
  return group(
    'pd-correction',
    text.profile.correctionTitle,
    [
      slot(ctx, {
        key: 'correction-import',
        label: text.profile.correctionFilter,
        card: current === null ? null : { name: baseName(current.path), path: current.path },
        describedBy: ids,
        off: locked,
        busy: importing || clearing,
        intent: 'import_correction',
        pick: () => ctx.shell.pickFile(text.profile.correctionDialog, { name: text.profile.correctionFilter, extensions: ['yml', 'yaml'] }),
        send: (value) => ctx.send({ kind: 'import_correction', value }),
        eject,
      }),
      current === null ? null : spec([[text.profile.correctionName, current.title]]),
      sr(text.profile.correctionBody, 'correction-about'),
      hint === null ? null : sr(hint, 'correction-hint'),
    ],
    'pd-sub',
  );
}

/**
 * What each device channel plays, as the printed patch strip by a
 * processor's outputs: a ruled row per channel with its number, an LED and
 * the speaker's name. The LED is lit while the speakers are the output in
 * use and the engine plays through that channel; `update` only lights them.
 */
function channels(ctx: Context): { element: HTMLElement; update(engine: EngineStatus): void } {
  const { profile, phase } = ctx.snapshot;
  const plays = phase.kind === 'running' && profile.kind === 'speakers';
  const lights: { channel: number; light: Led }[] = [];
  const rows = profile.speakers.channels.map((channel) => {
    const light = led('on', 'off', 'xs');
    lights.push({ channel: channel.channel, light });
    return h(
      'tr',
      null,
      h('td', { class: 'pd-num' }, channel.channel + 1),
      h('td', { class: 'pd-patch-led', 'aria-hidden': 'true' }, light.element),
      h('th', { scope: 'row' }, channel.name, channel.subwoofer ? h('span', { class: 'pd-tag' }, text.profile.subwoofer) : null),
    );
  });
  const element = group(
    'pd-channels',
    text.profile.channelsTitle,
    [
      h(
        'table',
        { class: 'pd-table pd-patch', 'aria-labelledby': 'pd-channels-title' },
        h(
          'thead',
          null,
          h('tr', null, h('th', { scope: 'col' }, text.profile.channel), h('td', { 'aria-hidden': 'true' }), h('th', { scope: 'col' }, text.profile.speaker)),
        ),
        h('tbody', null, ...rows),
      ),
    ],
    'pd-sub',
  );
  return {
    element,
    update(engine) {
      for (const { channel, light } of lights) light.set(plays && engine.output_rate !== 0 && channel < engine.output_channels ? 'on' : 'off');
    },
  };
}

function speakers(ctx: Context, patch: ReturnType<typeof channels>): Child[] {
  return [layout(ctx), correction(ctx), patch.element, ...applyLine(ctx, 'speakers')];
}

/** The LEVEL encoder with the range printed by it. */
function gain(scope: Scope): HTMLElement {
  return group('pd-gain', text.profile.gainTitle, [
    levelEncoder(scope, 64),
    print(`${signed(GAIN_MIN)} … ${signed(GAIN_MAX)} ${text.profile.gainUnit}`, 'pd-small pd-num'),
    manualRule(),
    manual(text.pedal.gainBody),
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
        // The angles are the manual's; the key prints the direction's name alone.
        detail: h('span', { class: 'pd-manual' }, dense ? `${Math.round(direction.azimuth_deg)}° / ${Math.round(direction.elevation_deg)}°` : angles),
        off: !running,
        locked: ctx.busy,
        busy: ctx.sending?.kind === 'test_direction' && ctx.sending.value === direction.id,
        intent: 'test_direction',
        describedBy: running ? null : 'test-hint',
        onClick: () => ctx.send({ kind: 'test_direction', value: direction.id }),
      });
      key.setAttribute('aria-label', `${direction.name}, ${angles}`);
      key.title = angles;
      return key;
    }),
  );
  return group('pd-tests', text.profile.testTitle, [
    manual(text.profile.testBody),
    manualRule(),
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
  const patch = channels(ctx);
  const update = (now: EngineStatus) => {
    map.update(now);
    patch.update(now);
  };
  if (engine !== null) update(engine);

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
          ...(open === 'headphones' ? headphones(ctx) : speakers(ctx, patch)),
        ),
        ...(running && !snapshot.profile.from_file ? [hairline(), fine(text.profile.importRunning, RUNNING_HINT)] : []),
      ]),
      flowRoom(),
    ),
    h(
      'div',
      { class: 'pd-profile-side' },
      gain(scope),
      group('pd-profile-map', text.profile.mapTitle, [manual(text.pedal.mapBody), manualRule(), map.element, running ? manual(text.pedal.mapTest) : null]),
      tests(ctx),
    ),
  );
  return running ? { element, live: { update } } : { element };
}
