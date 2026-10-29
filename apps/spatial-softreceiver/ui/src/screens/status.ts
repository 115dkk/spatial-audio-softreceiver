// The status screen (ready, running, faulted), one column read from the top.
// First the summary line: the power switch, the state word, the output device
// (with its list while off), the output kind and profile that play, and while
// running whether the sound has dropped out. Off, one quiet line names the
// three entrances. Running, one row per entrance (with its upmixer while one
// runs) and the ASIO input's row while it is on, then the measurements in
// three groups (stability, delay, output settings) with the internal
// counters folded under "details". The rows and the measurements are built
// once per snapshot and then only their text nodes change, 30 times a second.
// With the app-sound entrance (ADR 0043) the summary names it and, off or
// running, the lanes with their apps and pickers stand where the driver's
// entrances would.

import type { AsioStatus, EndpointStatus, EndpointUpmixView, EngineStatus, Snapshot } from '../bindings';
import { h, liveText, setAttr } from '../dom';
import {
  chainErrorEntries,
  chainErrorTotal,
  count,
  describeCarrier,
  describeClock,
  describeFormat,
  describeOutputFormat,
  describeOutputMode,
  describeRate,
  describeUnsupported,
  describeVolume,
  oneDecimal,
} from '../format';
import { ASIO_DEFAULTS } from '../guard';
import { text } from '../text';
import { bypassReason } from './upmix';
import { type Context, type Drawn, type Live, button, described, intentButton } from './common';
import {
  CAPTURE_LANES,
  PASS_LANE,
  appMode,
  captureStateWord,
  captureTone,
  chooseApp,
  laneName,
  laneView,
  pickerKey,
  pickerOptions,
  pickersLocked,
} from './entrance';
import { powerState } from './nav';
import { outputList } from './wizard';

/** Endpoints the screen shows: 0, 1 and 2. Slot 3 is reserved in the contract. */
const SHOWN_ENDPOINTS = 3;

/** A label over its value, one stop of the summary line. */
function fact(className: string, label: string, ...value: (Node | string | null)[]): HTMLElement {
  return h(
    'div',
    { class: `summary-item ${className}` },
    h('dt', { class: 'summary-label' }, label),
    h('dd', { class: 'summary-value' }, ...value),
  );
}

/**
 * The output kind and the profile it plays: "헤드폰 · <BRIR set>",
 * "스피커 · 7.1", or the correction's title with a tag saying a SECS
 * correction decides the speakers. A profile.toml says so in its tag.
 */
function profileValue(snapshot: Snapshot): (Node | string)[] {
  const { profile } = snapshot;
  const tag = profile.from_file
    ? text.profile.fromFileTag
    : profile.kind === 'speakers' && profile.speakers.correction !== null
      ? text.profile.correctionTitle
      : null;
  return [
    h('strong', { class: 'profile-kind' }, text.profile.kindNames[profile.kind]),
    ' · ',
    h('span', { class: 'profile-name' }, profile.name),
    ...(tag === null ? [] : [' ', h('span', { class: 'tag' }, tag)]),
  ];
}

interface Summary {
  element: HTMLElement;
  update(engine: EngineStatus | null): void;
}

/**
 * The head: one line that says whether it is on, where the sound goes, what
 * it is rendered for and, while running, whether it has dropped out.
 */
function summary(ctx: Context): Summary {
  const s = text.status;
  const { snapshot } = ctx;
  const running = snapshot.phase.kind === 'running';
  const toggle = intentButton(
    ctx,
    running ? s.powerOff : s.powerOn,
    running ? { kind: 'disable' } : { kind: 'enable' },
    {
      key: 'power',
      variant: running ? 'secondary' : 'primary',
      className: 'power',
      pendingLabel: running ? s.powerTurningOff : s.powerTurningOn,
      busy: 'spinner',
    },
  );
  // While turning on the button's spoken name is already the state word.
  if (ctx.pending !== 'enable') toggle.setAttribute('aria-describedby', 'power-state');

  const power = powerState(snapshot, ctx.pending);
  const state = fact(
    'summary-state',
    s.stateLabel,
    h(
      'strong',
      { id: 'power-state', class: `state-word tone-${power.tone}` },
      h('span', { class: 'lamp', 'aria-hidden': 'true' }),
      power.label,
    ),
  );

  const canChange = snapshot.phase.kind === 'ready';
  const output = fact(
    'summary-output',
    s.output,
    h(
      'span',
      { class: `output-current${snapshot.chosen_output === null ? ' is-missing' : ''}` },
      snapshot.chosen_output?.name ?? s.noOutput,
    ),
    canChange
      ? button(
          ctx.view.outputsOpen ? s.closeOutputs : snapshot.chosen_output === null ? s.choose : s.change,
          () => ctx.setView({ outputsOpen: !ctx.view.outputsOpen }),
          { key: 'outputs-toggle', variant: 'quiet', className: 'outputs-toggle' },
        )
      : null,
  );

  const kind = fact('summary-profile', text.profile.outputTitle, ...profileValue(snapshot));
  // The app-sound entrance says so; the driver entrance's three entrances
  // say themselves under the summary.
  const apps = appMode(snapshot);
  const entrance = apps ? fact('summary-entrance', text.entrance.label, text.entrance.appSound) : null;

  // Whether the sound has dropped out since the engine started: the count
  // the stability group lists first, said here in words.
  const healthWord = liveText('-');
  const healthLamp = h('span', { class: 'lamp', 'aria-hidden': 'true' });
  const health = running ? fact('summary-health', s.health, healthLamp, h('span', null, healthWord.node)) : null;

  const element = h(
    'section',
    { class: 'panel summary' },
    h('div', { class: 'summary-line' }, toggle, h('dl', { class: 'summary-facts' }, state, output, kind, entrance, health)),
    // The default roles are the driver entrance's alone (ADR 0043).
    running && snapshot.roles_taken && !apps ? h('p', { class: 'summary-note' }, s.rolesTaken) : null,
  );
  if (canChange && ctx.view.outputsOpen) element.append(h('div', { class: 'status-outputs' }, outputList(ctx, 'status-output')));
  return {
    element,
    update(engine) {
      if (health === null || engine === null) return;
      const dropped = engine.underruns > 0;
      healthWord.set(dropped ? s.asioUnderruns(count(engine.underruns)) : s.healthClean);
      setAttr(health, 'class', `summary-item summary-health tone-${dropped ? 'warning' : 'clean'}`);
    },
  };
}

/** Off or stopped: the three entrances by name, on one quiet line. */
function entranceLine(): HTMLElement {
  const names = text.endpoints.slice(0, SHOWN_ENDPOINTS).map((endpoint) => endpoint.name);
  const list = h('span', { class: 'entrance-line-names' });
  names.forEach((name, index) => {
    if (index > 0) list.append(h('span', { class: 'entrance-line-sep', 'aria-hidden': 'true' }, ' · '));
    list.append(h('span', { class: 'entrance-line-name' }, name));
  });
  return h('p', { class: 'entrance-line' }, h('span', { class: 'entrance-line-label' }, text.status.entrances), ' ', list);
}

/**
 * A lane's app picker (ADR 0043): the apps with a sound session, the lane's
 * own app when it is not running, and "none" on the music lane. Choosing one
 * sends `choose_capture_app`; locked while the engine runs.
 */
function lanePicker(ctx: Context, lane: number, locked: string | null): HTMLSelectElement {
  const current = laneView(ctx.snapshot, lane);
  const select = h('select', {
    id: pickerKey(lane),
    'data-key': pickerKey(lane),
    'data-intent': 'choose_capture_app',
    disabled: locked !== null || ctx.busy,
    'aria-busy': ctx.sending?.kind === 'choose_capture_app' && ctx.sending.value.lane === lane ? 'true' : null,
    'aria-labelledby': `capture-lane-${lane} capture-app-head`,
    'aria-describedby': described(locked !== null && 'capture-locked', 'capture-closed'),
  });
  ctx.offer('choose_capture_app');
  if (current === null && lane === 0) {
    select.append(h('option', { value: '', selected: true, disabled: true }, text.entrance.chooseApp));
  }
  for (const option of pickerOptions(ctx.snapshot, lane)) {
    select.append(
      h(
        'option',
        { value: option.exe ?? '', selected: option.exe === (current?.exe ?? null), disabled: option.blocked !== null },
        option.label,
      ),
    );
  }
  select.addEventListener('change', () => {
    const exe = select.value === '' ? null : select.value;
    if (exe === (current?.exe ?? null)) return;
    ctx.send(chooseApp(lane, exe));
  });
  return select;
}

/** A lane's state: a lamp and the word, or a dash with no app chosen. */
function laneState(ctx: Context, lane: number): HTMLElement {
  const view = laneView(ctx.snapshot, lane);
  if (view === null) return h('span', { class: 'muted' }, '-');
  return h(
    'span',
    { class: `capture-state tone-${captureTone(view.state)}` },
    h('span', { class: 'lamp', 'aria-hidden': 'true' }),
    captureStateWord(view.state),
  );
}

/**
 * The app-sound entrance's lanes (ADR 0043) in place of the driver's
 * entrances: the main and music lanes with their app, its picker and its
 * state, the PASS lane said to be the driver's, and under them what happens
 * to an app closed before this program turns off.
 */
function captureLanes(ctx: Context): HTMLElement {
  const e = text.entrance;
  const locked = pickersLocked(ctx.snapshot);
  const rows = [...CAPTURE_LANES, PASS_LANE].map((lane) =>
    h(
      'tr',
      { 'data-lane': lane },
      h(
        'th',
        { scope: 'row', class: 'capture-lane' },
        h('span', { class: `lane-dot lane-${lane}`, 'aria-hidden': 'true' }),
        h('span', { id: `capture-lane-${lane}` }, laneName(lane)),
      ),
      ...(lane === PASS_LANE
        ? [h('td', { class: 'muted', colspan: 2 }, e.driverOnly)]
        : [h('td', { class: 'capture-app' }, lanePicker(ctx, lane, locked)), h('td', null, laneState(ctx, lane))]),
    ),
  );
  return h(
    'section',
    { class: 'panel capture-lanes', 'aria-labelledby': 'capture-title' },
    h('h2', { id: 'capture-title', class: 'section-title' }, e.appsTitle),
    h(
      'table',
      { class: 'routing-table capture-table', 'aria-labelledby': 'capture-title' },
      h(
        'thead',
        null,
        h(
          'tr',
          null,
          h('th', { scope: 'col' }, e.lane),
          h('th', { scope: 'col', id: 'capture-app-head' }, text.settings.routingApp),
          h('th', { scope: 'col' }, text.status.stateLabel),
        ),
      ),
      h('tbody', null, ...rows),
    ),
    // Why the pickers are locked comes first, next to them.
    locked === null ? null : h('p', { id: 'capture-locked', class: 'hint capture-hint' }, locked),
    h('p', { id: 'capture-closed', class: 'hint capture-hint' }, e.closedApp),
  );
}

interface Card {
  element: HTMLElement;
  update(endpoint: EndpointStatus | null): void;
}

/**
 * The entrance's upmixer while its host is up: the plugin's name, a warning
 * tone when the original stereo plays in its place, and, in words under the
 * name, why the sound playing now goes around it (the lamp unlit).
 */
function upmixLine(upmix: EndpointUpmixView | undefined): HTMLElement | null {
  if (upmix === undefined || upmix.plugin === null) return null;
  const { kind } = upmix.state;
  if (kind !== 'running' && kind !== 'fallback') return null;
  const fallback = kind === 'fallback';
  const bypass = kind === 'running' ? upmix.bypass : null;
  const tone = fallback ? 'warning' : bypass !== null ? 'off' : 'on';
  return h(
    'p',
    { class: `card-upmix tone-${tone}` },
    h('span', { class: 'lamp', 'aria-hidden': 'true' }),
    h(
      'span',
      { class: 'card-upmix-text' },
      h('span', null, fallback ? text.status.upmixFallback(upmix.plugin.name) : text.status.upmix(upmix.plugin.name)),
      bypass === null ? null : h('span', { class: 'card-upmix-reason' }, bypassReason(bypass)),
    ),
  );
}

/**
 * One entrance as a row: its name and purpose on the left, and on the right
 * the format coming in (the value read first), the carrier of a bitstream,
 * the upmixer, why a format cannot play, the volume and the processing
 * errors, which open into their kinds.
 */
function endpointCard(index: number, upmix: EndpointUpmixView | undefined): Card {
  const s = text.status;
  const names = text.endpoints[index] ?? { name: `${index}`, purpose: '' };
  const titleId = `entrance-${index}-title`;
  const format = liveText();
  const carrier = liveText();
  const volume = liveText();
  const unsupported = liveText();
  const errorTotal = liveText();

  const carrierLine = h('p', { class: 'card-carrier', hidden: true }, carrier.node);
  const unsupportedLine = h('p', { class: 'card-warning', hidden: true }, unsupported.node);
  const errorList = h('ul', { class: 'error-kinds' });
  const errors = h(
    'details',
    { class: 'card-errors' },
    h(
      'summary',
      null,
      h('span', { class: 'card-meta-label' }, s.chainErrors),
      h('span', { class: 'card-meta-value num' }, errorTotal.node),
    ),
    errorList,
  );
  const rows = h(
    'dl',
    { class: 'card-rows' },
    h(
      'div',
      { class: 'fact' },
      h('dt', { class: 'card-meta-label' }, s.volume),
      h('dd', { class: 'card-meta-value num' }, volume.node),
    ),
  );
  const meta = h('div', { class: 'card-meta' }, rows, errors);
  const element = h(
    'article',
    { class: 'endpoint-card', 'aria-labelledby': titleId },
    h(
      'header',
      { class: 'card-head' },
      h('h3', { id: titleId, class: 'card-title' }, names.name),
      h('p', { class: 'card-purpose' }, names.purpose),
    ),
    h(
      'div',
      { class: 'card-body' },
      h('p', { class: 'card-format' }, format.node),
      carrierLine,
      upmixLine(upmix),
      unsupportedLine,
      meta,
    ),
  );

  let errorKey = '';
  return {
    element,
    update(endpoint) {
      element.classList.toggle('is-off', endpoint === null);
      meta.hidden = endpoint === null;
      if (endpoint === null) {
        format.set(s.engineOff);
        carrierLine.hidden = true;
        unsupportedLine.hidden = true;
        return;
      }
      format.set(describeFormat(endpoint.format));
      const container = describeCarrier(endpoint.format);
      carrierLine.hidden = container === null;
      if (container !== null) carrier.set(container);
      element.classList.toggle('is-idle', endpoint.format === null);
      volume.set(describeVolume(endpoint.volume_db, endpoint.muted));
      unsupportedLine.hidden = endpoint.unsupported === null;
      if (endpoint.unsupported !== null) unsupported.set(describeUnsupported(endpoint.unsupported));

      const entries = chainErrorEntries(endpoint.chain_errors);
      const total = chainErrorTotal(endpoint.chain_errors);
      errorTotal.set(total === 0 ? s.chainErrorsNone : count(total));
      errors.classList.toggle('has-errors', total > 0);
      // The kinds change rarely; rebuild the small list only when they do.
      const key = entries.map((entry) => `${entry.kind}:${entry.count}`).join(',');
      if (key !== errorKey) {
        errorKey = key;
        errorList.replaceChildren(
          ...entries.map((entry) =>
            h('li', null, h('span', null, entry.label), h('span', { class: 'num' }, count(entry.count))),
          ),
        );
      }
      if (total === 0) errors.open = false;
    },
  };
}

interface AsioLine {
  element: HTMLElement;
  update(status: AsioStatus | null): void;
}

/**
 * The ASIO input (ADR 0025) as the last entrance row, its name on the left
 * like the entrances'. A DAW that holds it: its program file, the rate, the
 * buffer and its length, the layout it sends and the times the output found
 * nothing from it, with a warning while a stand-in clock drives the DAW.
 * None: one quiet line. A fault: its reason as the engine gives it.
 */
function asioLine(): AsioLine {
  const s = text.status;
  const client = liveText();
  const rate = liveText();
  const buffer = liveText();
  const layout = liveText();
  const underruns = liveText();
  const reason = liveText();
  const details = [rate, buffer, layout, underruns].map((value) => h('span', { class: 'asio-detail' }, value.node));
  const state = h(
    'p',
    { class: 'asio-state' },
    h('span', { class: 'lamp', 'aria-hidden': 'true' }),
    h('span', { class: 'asio-state-text' }, h('span', { class: 'asio-client' }, client.node), ...details),
  );
  const clockWarning = h('p', { class: 'card-warning', hidden: true }, s.asioClockFault);
  const fault = h('p', { class: 'asio-fault', hidden: true }, reason.node);
  const element = h(
    'article',
    { class: 'asio-line', 'aria-labelledby': 'asio-line-title' },
    h('header', { class: 'card-head' }, h('h3', { id: 'asio-line-title', class: 'card-title' }, text.asio)),
    h('div', { class: 'card-body' }, state, clockWarning, fault),
  );
  return {
    element,
    update(status) {
      const faultReason = status?.fault_reason ?? null;
      // A fault comes with no DAW to speak of; the reason is the line.
      const attached = status !== null && faultReason === null ? status : null;
      const tone = faultReason !== null ? 'error' : attached === null ? 'off' : attached.clock_fault ? 'warning' : 'on';
      setAttr(state, 'class', `asio-state tone-${tone}`);
      client.set(faultReason !== null ? s.asioFailed : attached === null ? s.asioWaiting : attached.client_exe || '-');
      for (const detail of details) detail.hidden = attached === null;
      if (attached !== null) {
        const { sample_rate: hz, buffer_frames: frames } = attached;
        rate.set(hz > 0 ? describeRate(hz) : '-');
        buffer.set(s.asioBuffer(count(frames), hz > 0 ? oneDecimal((frames / hz) * 1000) : '-'));
        layout.set(`${attached.layout} (${text.format.channels(attached.channels)})`);
        underruns.set(s.asioUnderruns(count(attached.underruns)));
      }
      clockWarning.hidden = attached === null || !attached.clock_fault;
      fault.hidden = faultReason === null;
      if (faultReason !== null) reason.set(faultReason);
    },
  };
}

type MeterRow = [label: string, unit: string, read: (engine: EngineStatus) => string];

/** The measurements by what they answer: has it dropped out, how late is it, how does it play. */
function meterGroups(): [title: string, rows: MeterRow[]][] {
  const s = text.status;
  const m = text.meters;
  return [
    [
      s.health,
      [
        [m.underruns, m.times, (e) => count(e.underruns)],
        [m.outputRestarts, m.times, (e) => count(e.output_restarts)],
        [m.decodeRefused, m.times, (e) => count(e.decode_refused)],
      ],
    ],
    [
      s.latency,
      [
        [m.latencyP50, m.ms, (e) => (e.latency.samples === 0 ? '-' : oneDecimal(e.latency.p50_ms))],
        [m.latencyP99, m.ms, (e) => (e.latency.samples === 0 ? '-' : oneDecimal(e.latency.p99_ms))],
        [m.ringLevel, m.ms, (e) => oneDecimal(e.ring_level_ms)],
      ],
    ],
    [
      s.outputSettings,
      [
        [m.outputMode, '', describeOutputMode],
        [m.output, '', describeOutputFormat],
        [m.clock, '', describeClock],
      ],
    ],
  ];
}

/** One measurement row; returns its setter. */
function meterRow(list: HTMLElement, [label, unit, read]: MeterRow): (engine: EngineStatus) => void {
  const value = liveText();
  list.append(
    h(
      'div',
      { class: 'meter' },
      h('dt', null, label),
      h('dd', null, h('span', { class: 'num' }, value.node), unit ? h('span', { class: 'unit' }, unit) : null),
    ),
  );
  return (engine) => value.set(read(engine));
}

function meterPanel(apps: boolean): { element: HTMLElement; update(engine: EngineStatus): void } {
  const s = text.status;
  const m = text.meters;
  const setters: ((engine: EngineStatus) => void)[] = [];
  const groups = meterGroups().map(([title, rows], index) => {
    const list = h('dl', { class: 'meter-list' });
    for (const row of rows) setters.push(meterRow(list, row));
    return h(
      'section',
      { class: 'meter-group', 'aria-labelledby': `meter-group-${index}` },
      h('h3', { id: `meter-group-${index}`, class: 'meter-group-title' }, title),
      list,
    );
  });

  // Counters for a report rather than for listening: the bytes the driver
  // lost and how many times each entrance's format changed.
  const lost = h('dl', { class: 'meter-list' });
  setters.push(meterRow(lost, [m.lostBytes, m.bytes, (e) => count(e.lost_bytes)]));
  const epochs = h('dl', { class: 'meter-list', 'aria-labelledby': 'meter-epoch-title', 'aria-describedby': 'meter-epoch-hint' });
  // The app-sound entrance numbers its lanes like the endpoints; PASS is unused there.
  for (const index of apps ? CAPTURE_LANES : [0, 1, 2]) {
    const name = apps ? laneName(index) : (text.endpoints[index]?.name ?? String(index));
    const read = (e: EngineStatus) => {
      const endpoint = e.endpoints[index];
      return endpoint === undefined ? '-' : count(endpoint.epoch);
    };
    setters.push(meterRow(epochs, [name, '', read]));
  }
  const more = h(
    'details',
    { class: 'meter-more' },
    h('summary', null, s.details),
    h(
      'div',
      { class: 'meter-more-body' },
      lost,
      h(
        'section',
        { class: 'meter-epochs' },
        h('h4', { id: 'meter-epoch-title', class: 'meter-sub-title' }, s.epoch),
        h('p', { id: 'meter-epoch-hint', class: 'meter-sub-hint' }, s.epochHint),
        epochs,
      ),
    ),
  );

  const element = h(
    'section',
    { class: 'panel meter-panel', 'aria-labelledby': 'meters-title' },
    h('h2', { id: 'meters-title', class: 'section-title' }, s.meters),
    h('div', { class: 'meters' }, ...groups),
    more,
  );
  return {
    element,
    update(engine) {
      for (const set of setters) set(engine);
    },
  };
}

export function status(ctx: Context, engineNow: EngineStatus | null): Drawn {
  const s = text.status;
  const running = ctx.snapshot.phase.kind === 'running';
  const head = summary(ctx);

  // The app-sound entrance has lanes with apps, not the driver's endpoints.
  const apps = appMode(ctx.snapshot);

  if (!running) {
    // Off or stopped there is nothing live to show: what would play where is
    // the summary, and the entrances are names (or the lanes and their apps).
    const element = h('div', { class: 'status is-off' }, head.element, apps ? captureLanes(ctx) : entranceLine());
    return { element };
  }

  const cards = apps
    ? []
    : Array.from({ length: SHOWN_ENDPOINTS }, (_, index) =>
        endpointCard(
          index,
          ctx.snapshot.upmix.endpoints.find((endpoint) => endpoint.endpoint === index),
        ),
      );
  // The ASIO row stands while the input is on and the engine runs.
  const asio = (ctx.snapshot.asio ?? ASIO_DEFAULTS).enabled ? asioLine() : null;
  const entrances =
    apps && asio === null
      ? null
      : h(
          'section',
          { class: 'panel entrances', 'aria-labelledby': 'entrances-title' },
          h('h2', { id: 'entrances-title', class: 'section-title' }, s.entrances),
          cards.length === 0 ? null : h('div', { class: 'entrance-cards' }, ...cards.map((card) => card.element)),
          asio?.element ?? null,
        );
  const meters = meterPanel(apps);
  const live: Live = {
    update(engine) {
      head.update(engine);
      cards.forEach((card, index) => card.update(engine.endpoints[index] ?? null));
      asio?.update(engine.asio ?? null);
      meters.update(engine);
    },
  };
  if (engineNow !== null) live.update(engineNow);
  else {
    for (const card of cards) card.update(null);
    asio?.update(null);
  }

  const element = h(
    'div',
    { class: 'status is-running' },
    head.element,
    apps ? captureLanes(ctx) : null,
    entrances,
    meters.element,
  );
  return engineNow === null ? { element } : { element, live };
}
