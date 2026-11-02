// The status face (ADR 0046). Running: each input's channel hangs under its
// jack (the endpoint, its format and carrier printed, what went wrong by a
// red or amber LED, an LED bar graph from its loudest sound and the
// endpoint's volume on a segment readout), the sound map in the middle and
// the output on the right (the LEVEL encoder, the output kind's slide switch
// and what plays where). The numbers that say whether the sound is healthy
// run along a row of segment readouts under them, the counters and each
// input's details after. Off or stopped: what will play where, the output
// devices' key and row, and the inputs by name and purpose. With the
// app-sound entrance (ADR 0043) the channels are its lanes, each with the key
// that opens its row of apps, and PASS stands dark.

import type { AsioStatus, EndpointUpmixView, EngineStatus } from '../../bindings';
import { type Child, h, liveText, setAttr } from '../../dom';
import {
  chainErrorEntries,
  chainErrorTotal,
  count,
  describeCarrier,
  describeFormat,
  describeOutputFormat,
  describeOutputMode,
  describeRate,
  describeUnsupported,
  describeVolume,
  oneDecimal,
} from '../../format';
import { ASIO_DEFAULTS } from '../../guard';
import { text } from '../../text';
import type { Drawn, Live } from '../../screens/common';
import {
  CAPTURE_LANES,
  PASS_LANE,
  appLabel,
  appMode,
  captureStateWord,
  chooseApp,
  laneName,
  laneView,
  pickerKey,
  pickerOptions,
  pickersLocked,
} from '../../screens/entrance';
import { bypassReason } from '../../screens/upmix';
import { kindSlide, levelEncoder, outputRow } from './controls';
import { soundMap } from './map';
import { etchedField, facts, fine, flowArrow, ikey, keyRow, ladder, led, print, readout, sr, strength } from './parts';
import { ASIO_LANE, LANES, type Scope, loudest, signed } from './scope';

/** What the output in use is called: the BRIR set, the layout or the correction, or profile.toml's name. */
export function profileFact(scope: Scope): [string, string] {
  const { profile } = scope.ctx.snapshot;
  if (profile.from_file) return [text.profile.fromFileTag, profile.name];
  if (profile.kind === 'headphones') return [text.profile.brir, profile.name];
  return [profile.speakers.correction === null ? text.profile.layout : text.profile.correctionTitle, profile.name];
}

interface Channel {
  element: HTMLElement;
  /** The facts for the details: the format change number and the chain errors by kind. */
  details: HTMLElement[];
  update(engine: EngineStatus): void;
}

/** A printed note in a channel with its LED. */
const note = (tone: 'on' | 'wait' | 'fault' | 'off', ...words: Child[]) =>
  h('p', { class: 'pd-note' }, led(tone === 'off' ? 'on' : tone, tone === 'off' ? 'off' : 'on', 'sm').element, h('span', null, ...words));

/** The upmixer's line while its host is up: on, the original stereo in its place, or gone around. */
function upmixNote(upmix: EndpointUpmixView | undefined): HTMLElement | null {
  if (upmix === undefined || upmix.plugin === null) return null;
  const { kind } = upmix.state;
  if (kind !== 'running' && kind !== 'fallback') return null;
  const fallback = kind === 'fallback';
  const bypass = kind === 'running' ? upmix.bypass : null;
  return note(
    fallback ? 'wait' : bypass !== null ? 'off' : 'on',
    fallback ? text.status.upmixFallback(upmix.plugin.name) : text.status.upmix(upmix.plugin.name),
    bypass === null ? null : h('span', { class: 'pd-note-reason' }, bypassReason(bypass)),
  );
}

/** The chain errors by kind as printed lines, one prebuilt per known kind and one for the rest. */
function errorKinds(): { element: HTMLElement; set(map: EngineStatus['endpoints'][number]['chain_errors']): void } {
  const known = Object.keys(text.chainErrorKinds);
  const lines = known.map((kind) => {
    const value = liveText();
    const element = h('li', { hidden: true }, h('span', null, text.chainErrorKinds[kind] ?? kind), h('span', { class: 'pd-num' }, value.node));
    return { kind, element, value };
  });
  const rest = liveText();
  const restLine = h('li', { hidden: true }, rest.node);
  const element = h('ul', { class: 'pd-kinds' }, ...lines.map((line) => line.element), restLine);
  return {
    element,
    set(map) {
      const entries = chainErrorEntries(map);
      for (const line of lines) {
        const entry = entries.find((candidate) => candidate.kind === line.kind);
        if (line.element.hidden !== (entry === undefined)) line.element.hidden = entry === undefined;
        if (entry !== undefined) line.value.set(count(entry.count));
      }
      const others = entries.filter((entry) => !known.includes(entry.kind));
      restLine.hidden = others.length === 0;
      rest.set(others.map((entry) => `${entry.label} ${count(entry.count)}`).join(' · '));
    },
  };
}

/**
 * One input's channel. `label` names an app-sound lane and its app in place
 * of the driver's endpoint (ADR 0043).
 */
function channel(scope: Scope, lane: number, label?: { name: string; sub: string }): Channel {
  const s = text.status;
  const name = text.pedal.lamps[lane] ?? String(lane);
  const endpoint = label === undefined ? (text.endpoints[lane]?.name ?? name) : `${label.name} · ${label.sub}`;
  const format = liveText(s.noInput);
  const carrier = liveText();
  const unsupported = liveText();
  const errorTotal = liveText();
  const epoch = liveText();
  const carrierLine = h('p', { class: 'pd-small', hidden: true }, carrier.node);
  const unsupportedLine = note('wait', unsupported.node);
  unsupportedLine.hidden = true;
  const errorLine = note('fault', s.chainErrors, ' ', h('span', { class: 'pd-num' }, errorTotal.node));
  errorLine.hidden = true;
  const bar = ladder(`lane-${lane}`);
  const volume = readout(4, 18, 'dB', '', 'pd-ch-volume');
  const muted = print(s.muted, 'pd-tag');
  muted.hidden = true;
  const element = h(
    'section',
    { class: 'pd-ch', 'data-lane': lane, 'data-signal': 'none', 'aria-labelledby': `pd-ch-${lane}` },
    h('h3', { id: `pd-ch-${lane}`, class: 'pd-ep' }, sr(`${name} · `), endpoint),
    h('div', { class: 'pd-ch-format' }, h('p', { class: 'pd-fmt' }, format.node), carrierLine),
    h('div', { class: 'pd-ch-notes' }, upmixNote(scope.ctx.snapshot.upmix.endpoints.find((view) => view.endpoint === lane)), unsupportedLine, errorLine),
    h('div', { class: 'pd-ladder-row' }, bar.element, h('div', { class: 'pd-ch-level' }, muted, volume.element)),
  );
  const kinds = errorKinds();
  const total = liveText();
  const details = [
    h('div', { class: 'pd-fact' }, h('dt', null, `${name} · ${s.epoch}`), h('dd', { class: 'pd-num', title: s.epochHint }, epoch.node)),
    h('div', { class: 'pd-fact' }, h('dt', null, `${name} · ${s.chainErrors}`), h('dd', null, h('span', { class: 'pd-num' }, total.node), kinds.element)),
  ];
  return {
    element,
    details,
    update(engine) {
      const status = engine.endpoints[lane];
      if (status === undefined) return;
      const level = loudest(engine, lane);
      const sounding = level > -Infinity;
      setAttr(element, 'data-signal', sounding ? 'sound' : status.format !== null ? 'idle' : 'none');
      bar.set(sounding ? strength(level) : 0);
      format.set(describeFormat(status.format));
      const container = describeCarrier(status.format);
      carrierLine.hidden = container === null;
      if (container !== null) carrier.set(container);
      volume.set(signed(status.volume_db), describeVolume(status.volume_db, status.muted));
      muted.hidden = !status.muted;
      unsupportedLine.hidden = status.unsupported === null;
      if (status.unsupported !== null) unsupported.set(describeUnsupported(status.unsupported));
      const errors = chainErrorTotal(status.chain_errors);
      errorLine.hidden = errors === 0;
      errorTotal.set(count(errors));
      total.set(errors === 0 ? s.chainErrorsNone : count(errors));
      epoch.set(count(status.epoch));
      kinds.set(status.chain_errors);
    },
  };
}

/** The ASIO input's channel while it is on: the DAW, its rate, buffer, layout and underruns, or why not. */
function asioChannel(enabled: boolean): { element: HTMLElement; update(status: AsioStatus | null, engine: EngineStatus | null): void } {
  const s = text.status;
  const name = text.pedal.lamps[ASIO_LANE] ?? text.asio;
  const client = liveText(enabled ? s.asioWaiting : text.facts.off);
  const detail = liveText();
  const reason = liveText();
  const detailLine = h('p', { class: 'pd-small', hidden: true }, detail.node);
  const clock = note('wait', s.asioClockFault);
  clock.hidden = true;
  const fault = note('fault', reason.node);
  fault.hidden = true;
  const bar = ladder(`lane-${ASIO_LANE}`);
  const element = h(
    'section',
    { class: `pd-ch${enabled ? '' : ' is-dark'}`, 'data-lane': ASIO_LANE, 'data-signal': 'none', 'aria-labelledby': 'pd-ch-asio' },
    h('h3', { id: 'pd-ch-asio', class: 'pd-ep' }, sr(`${name} · `), text.asio),
    h('div', { class: 'pd-ch-format' }, h('p', { class: 'pd-fmt' }, client.node), detailLine),
    h('div', { class: 'pd-ch-notes' }, clock, fault),
    h('div', { class: 'pd-ladder-row' }, bar.element),
  );
  return {
    element,
    update(status, engine) {
      if (!enabled) return;
      const faultReason = status?.fault_reason ?? null;
      const attached = status !== null && faultReason === null ? status : null;
      setAttr(element, 'data-signal', attached === null ? 'none' : attached.clock_fault ? 'idle' : 'sound');
      client.set(faultReason !== null ? s.asioFailed : attached === null ? s.asioWaiting : attached.client_exe || '-');
      detailLine.hidden = attached === null;
      if (attached !== null) {
        const { sample_rate: hz, buffer_frames: frames } = attached;
        detail.set(
          [
            hz > 0 ? describeRate(hz) : '-',
            s.asioBuffer(count(frames), hz > 0 ? oneDecimal((frames / hz) * 1000) : '-'),
            `${attached.layout} (${text.format.channels(attached.channels)})`,
            s.asioUnderruns(count(attached.underruns)),
          ].join(' · '),
        );
      }
      clock.hidden = attached === null || !attached.clock_fault;
      fault.hidden = faultReason === null;
      if (faultReason !== null) reason.set(faultReason);
      const level = engine === null ? -Infinity : loudest(engine, ASIO_LANE);
      bar.set(level > -Infinity ? strength(level) : 0);
    },
  };
}

/**
 * A lane's app key (ADR 0043): an illuminated key with the app's name, its
 * ring lit while the lane's row of apps stands open under the channels.
 * Choosing a key in the row sends `choose_capture_app` and closes it. Off
 * while the engine runs, beside the reason.
 */
function laneKey(scope: Scope, lane: number): { key: HTMLElement; row: HTMLElement | null } {
  const { ctx, picker } = scope;
  const e = text.entrance;
  const view = laneView(ctx.snapshot, lane);
  const locked = pickersLocked(ctx.snapshot);
  const open = picker.open === lane && locked === null;
  ctx.offer('choose_capture_app');
  const sending = ctx.sending?.kind === 'choose_capture_app' && ctx.sending.value.lane === lane;
  const key = ikey({
    key: pickerKey(lane),
    label: view === null ? (lane === 0 ? e.chooseApp : text.profile.none) : appLabel(view),
    detail: view === null ? undefined : captureStateWord(view.state),
    lit: open,
    beside: true,
    off: locked !== null,
    locked: ctx.busy,
    busy: sending,
    intent: 'choose_capture_app',
    describedBy: locked === null ? 'capture-closed' : 'capture-locked capture-closed',
    attrs: { 'aria-expanded': open ? 'true' : 'false' },
    onClick: () => picker.set(open ? null : lane),
  });
  if (!open) return { key, row: null };
  const row = keyRow<string | null>({
    label: laneName(lane),
    choices: pickerOptions(ctx.snapshot, lane).map((option, index) => ({
      value: option.exe,
      label: option.label,
      key: `${pickerKey(lane)}-${index}`,
      off: option.blocked !== null,
      detail: option.blocked ?? undefined,
    })),
    chosen: view?.exe ?? null,
    off: false,
    locked: ctx.busy,
    busy: sending,
    intent: 'choose_capture_app',
    choose: (exe) => {
      ctx.send(chooseApp(lane, exe));
      picker.set(null);
    },
  });
  row.setAttribute('data-region', `capture-apps-${lane}`);
  return { key, row: h('div', { class: 'pd-under-row' }, print(laneName(lane)), row) };
}

/** An app-sound lane without live numbers: its name and its app key. */
function laneStandby(lane: number, key: HTMLElement): HTMLElement {
  return h(
    'section',
    { class: 'pd-ch is-standby', 'data-lane': lane, 'data-signal': 'none', 'aria-labelledby': `pd-ch-${lane}` },
    h('h3', { id: `pd-ch-${lane}`, class: 'pd-ep' }, laneName(lane)),
    h('div', { class: 'pd-ch-format' }, key),
  );
}

/** The PASS lane with the app-sound entrance: the driver's alone, so dark. */
function passLane(): HTMLElement {
  return h(
    'section',
    { class: 'pd-ch is-dark', 'data-lane': PASS_LANE, 'data-signal': 'none', 'aria-labelledby': `pd-ch-${PASS_LANE}` },
    h('h3', { id: `pd-ch-${PASS_LANE}`, class: 'pd-ep' }, laneName(PASS_LANE)),
    h('div', { class: 'pd-ch-format' }, h('p', { class: 'pd-fmt' }, text.entrance.driverOnly)),
  );
}

/** Why the app keys are off (running), and what happens to an app closed first. */
function laneHints(scope: Scope): HTMLElement[] {
  const locked = pickersLocked(scope.ctx.snapshot);
  return [...(locked === null ? [] : [fine(locked, 'capture-locked')]), fine(text.entrance.closedApp, 'capture-closed')];
}

/** A segment readout in the meters row: its printed name, the digits and the unit. */
function meter(
  label: (engine: EngineStatus) => string,
  read: (engine: EngineStatus) => string,
  unit: string,
  cells: number,
  height: number,
  warn?: (engine: EngineStatus) => boolean,
) {
  const name = liveText();
  const digits = readout(cells, height, unit);
  const lamp = warn === undefined ? null : led('wait', 'off', 'sm');
  const element = h('div', { class: 'pd-meter' }, h('span', { class: 'pd-label' }, name.node, lamp?.element ?? null), digits.element);
  return {
    element,
    update(engine: EngineStatus) {
      name.set(label(engine));
      digits.set(read(engine));
      lamp?.set(warn?.(engine) ? 'on' : 'off');
    },
  };
}

/** The input columns, the map and the output on one grid under the jacks. */
function columns(inputs: Child[], middle: Child[], output: Child[]): HTMLElement {
  return h(
    'div',
    { class: 'pd-cols pd-status-main' },
    h('div', { class: 'pd-chs' }, ...inputs),
    flowArrow(),
    h('div', { class: 'pd-mapcol' }, ...middle),
    flowArrow(),
    h('div', { class: 'pd-outcol' }, ...output),
  );
}

function running(scope: Scope, engineNow: EngineStatus | null): Drawn {
  const { ctx } = scope;
  const { snapshot } = ctx;
  const m = text.meters;
  const s = text.status;
  const apps = appMode(snapshot);
  const asioOn = (snapshot.asio ?? ASIO_DEFAULTS).enabled;

  const channels: Channel[] = [];
  const under: HTMLElement[] = [];
  const inputs: HTMLElement[] = [];
  if (apps) {
    for (const lane of CAPTURE_LANES) {
      const view = laneView(snapshot, lane);
      const { key, row } = laneKey(scope, lane);
      if (row !== null) under.push(row);
      if (view === null) {
        inputs.push(laneStandby(lane, key));
      } else {
        const input = channel(scope, lane, { name: laneName(lane), sub: appLabel(view) });
        channels.push(input);
        input.element.querySelector('.pd-ch-format')?.append(key);
        inputs.push(input.element);
      }
    }
    inputs.push(passLane());
  } else {
    for (let lane = 0; lane < LANES; lane += 1) {
      const input = channel(scope, lane);
      channels.push(input);
      inputs.push(input.element);
    }
  }
  const asio = asioChannel(asioOn);
  inputs.push(asio.element);

  const map = soundMap(ctx, engineNow?.directions ?? snapshot.profile.directions, true, 'map-direction');
  const outputFormat = liveText();
  const outputMode = liveText();
  const [profileLabel, profileName] = profileFact(scope);
  const main = columns(
    inputs,
    [h('div', { class: 'pd-mapwrap' }, etchedField(), map.element), fine(text.pedal.mapTest, null, 'pd-centre')],
    [
      levelEncoder(scope),
      kindSlide(scope),
      facts([
        [profileLabel, profileName],
        [text.pedal.device, snapshot.chosen_output?.name ?? s.noOutput],
        [m.output, outputFormat.node],
        [m.outputMode, outputMode.node],
      ]),
      snapshot.roles_taken && !apps ? fine(s.rolesTaken) : null,
    ],
  );

  const named = (label: string) => () => label;
  const meters = [
    meter(named(m.latencyP50), (e) => (e.latency.samples === 0 ? '-' : oneDecimal(e.latency.p50_ms)), m.ms, 4, 22),
    meter(named(m.latencyP99), (e) => (e.latency.samples === 0 ? '-' : oneDecimal(e.latency.p99_ms)), m.ms, 4, 22),
    meter(named(m.ringLevel), (e) => oneDecimal(e.ring_level_ms), m.ms, 4, 22),
    meter(named(m.underruns), (e) => count(e.underruns), m.times, 4, 22, (e) => e.underruns > 0),
    // The clock's kind (trim or ASRC) by its name; the correction on the readout.
    meter((e) => `${m.clock} · ${e.clock === 'trim' ? m.trim : m.asrc}`, (e) => signed(e.clock_ppm), m.ppm, 5, 22),
  ];
  const counters = [
    meter(named(m.outputRestarts), (e) => count(e.output_restarts), m.times, 6, 14),
    meter(named(m.decodeRefused), (e) => count(e.decode_refused), m.times, 6, 14),
    meter(named(m.lostBytes), (e) => count(e.lost_bytes), m.bytes, 8, 14),
    meter(named(m.skippedRecords), (e) => count(e.skipped_records), m.times, 6, 14),
  ];

  const element = h(
    'div',
    { class: 'pd-status is-running' },
    main,
    under.length === 0 ? null : h('div', { class: 'pd-under' }, ...under),
    apps ? h('div', { class: 'pd-under pd-hints' }, ...laneHints(scope)) : null,
    h('div', { class: 'pd-rule', 'aria-hidden': 'true' }),
    h(
      'div',
      { class: 'pd-meters', role: 'group', 'aria-label': s.meters },
      ...meters.map((cell) => cell.element),
      h('span', { class: 'pd-sep', 'aria-hidden': 'true' }),
      h('div', { class: 'pd-counters' }, ...counters.map((cell) => cell.element)),
    ),
    h(
      'section',
      { class: 'pd-details', 'aria-labelledby': 'pd-details-title' },
      h('h2', { id: 'pd-details-title', class: 'pd-label' }, text.pedal.details),
      h('dl', { class: 'pd-facts pd-details-grid' }, ...channels.flatMap((input) => input.details)),
    ),
  );

  const live: Live = {
    update(engine) {
      for (const input of channels) input.update(engine);
      asio.update(engine.asio ?? null, engine);
      map.update(engine);
      outputFormat.set(describeOutputFormat(engine));
      outputMode.set(describeOutputMode(engine));
      for (const cell of meters) cell.update(engine);
      for (const cell of counters) cell.update(engine);
    },
  };
  if (engineNow !== null) live.update(engineNow);
  else asio.update(null, null);
  return engineNow === null ? { element } : { element, live };
}

/** Off or stopped: where it will play, the output devices while off, the inputs by name and purpose. */
function standby(scope: Scope): Drawn {
  const { ctx } = scope;
  const { snapshot, view } = ctx;
  const s = text.status;
  const canChange = snapshot.phase.kind === 'ready';
  const chosen = snapshot.chosen_output;
  const apps = appMode(snapshot);
  const asioOn = (snapshot.asio ?? ASIO_DEFAULTS).enabled;
  const [profileLabel, profileName] = profileFact(scope);

  const under: HTMLElement[] = [];
  const inputs: HTMLElement[] = [];
  if (apps) {
    for (const lane of CAPTURE_LANES) {
      const { key, row } = laneKey(scope, lane);
      if (row !== null) under.push(row);
      inputs.push(laneStandby(lane, key));
    }
    inputs.push(passLane());
  } else {
    text.endpoints.forEach((endpoint, lane) =>
      inputs.push(
        h(
          'section',
          { class: 'pd-ch is-standby', 'data-lane': lane, 'data-signal': 'none', 'aria-labelledby': `pd-ch-${lane}` },
          h('h3', { id: `pd-ch-${lane}`, class: 'pd-ep' }, sr(`${text.pedal.lamps[lane] ?? ''} · `), endpoint.name),
          h('div', { class: 'pd-ch-format' }, h('p', { class: 'pd-small' }, endpoint.purpose)),
        ),
      ),
    );
  }
  inputs.push(
    h(
      'section',
      { class: `pd-ch is-standby${asioOn ? '' : ' is-dark'}`, 'data-lane': ASIO_LANE, 'data-signal': 'none', 'aria-labelledby': 'pd-ch-asio' },
      h('h3', { id: 'pd-ch-asio', class: 'pd-ep' }, text.asio),
      h('div', { class: 'pd-ch-format' }, h('p', { class: 'pd-small' }, asioOn ? text.facts.on : text.facts.off)),
    ),
  );

  const plays: Child[] = [
    print(text.pedal.standbyDevice),
    h('p', { class: `pd-device${chosen === null ? ' is-none' : ''}` }, chosen?.name ?? s.noOutput),
  ];
  if (canChange) {
    plays.push(
      ikey({
        key: 'outputs-toggle',
        label: text.wizard.outputsLabel,
        lit: view.outputsOpen,
        beside: true,
        attrs: { 'aria-expanded': view.outputsOpen ? 'true' : 'false' },
        onClick: () => ctx.setView({ outputsOpen: !view.outputsOpen }),
      }),
    );
    if (view.outputsOpen) under.unshift(h('div', { class: 'pd-under-row' }, outputRow(ctx, 'status-output')));
  }

  const element = h(
    'div',
    { class: 'pd-status is-standby' },
    columns(inputs, [h('div', { class: 'pd-plays' }, ...plays)], [levelEncoder(scope), kindSlide(scope), facts([[profileLabel, profileName]])]),
    under.length === 0 ? null : h('div', { class: 'pd-under' }, ...under),
    apps ? h('div', { class: 'pd-under pd-hints' }, ...laneHints(scope)) : null,
  );
  return { element };
}

export function statusFace(scope: Scope, engineNow: EngineStatus | null): Drawn {
  return scope.ctx.snapshot.phase.kind === 'running' ? running(scope, engineNow) : standby(scope);
}
