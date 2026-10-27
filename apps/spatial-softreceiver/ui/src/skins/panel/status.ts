// The status page in the display. Running: the inputs, the sound position
// map and the output side by side, the five numbers that say whether the
// sound is healthy in a strip under them, and every other measurement in the
// details below. Off: a dim standby that says where and how it will play, the
// output list, and that the power button turns it on. Stopped: the reason
// stands in the band above, with the recheck row.

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
import { bypassReason } from '../../screens/upmix';
import { soundMap } from './map';
import { LANES, type Scope, signed } from './scope';
import { dot, facts, hint, kindIcon, outputRows, row, section } from './ui';

/** Levels at or under the floor show an empty bar. */
const FLOOR_DB = -60;

/** What the output in use is called: the BRIR set, the layout or the correction, or profile.toml's name. */
function profileFact(scope: Scope): [string, string] {
  const { profile } = scope.ctx.snapshot;
  if (profile.from_file) return [text.profile.fromFileTag, profile.name];
  if (profile.kind === 'headphones') return [text.profile.brir, profile.name];
  return [profile.speakers.correction === null ? text.profile.layout : text.profile.correctionTitle, profile.name];
}

/** The big gain numeral that follows the encoder. */
function gainNumeral(scope: Scope, className: string): HTMLElement {
  const value = liveText(signed(scope.gain.value()));
  scope.gain.follow((db) => value.set(signed(db)));
  return h('p', { class: `pn-big ${className}`.trim() }, h('span', { class: 'num' }, value.node), h('small', null, text.profile.gainUnit));
}

/** The kind in use with its icon. */
function kindLine(scope: Scope): HTMLElement {
  const { kind } = scope.ctx.snapshot.profile;
  return h('p', { class: 'pn-kind' }, kindIcon(kind), text.profile.kindNames[kind]);
}

interface InputRow {
  element: HTMLElement;
  /** The facts in the details: the format change number and the chain errors by kind. */
  details: HTMLElement[];
  update(engine: EngineStatus): void;
}

/** The upmixer's line while its host is up: on, the original stereo in its place, or gone around. */
function upmixLine(upmix: EndpointUpmixView | undefined): HTMLElement | null {
  if (upmix === undefined || upmix.plugin === null) return null;
  const { kind } = upmix.state;
  if (kind !== 'running' && kind !== 'fallback') return null;
  const fallback = kind === 'fallback';
  const bypass = kind === 'running' ? upmix.bypass : null;
  return h(
    'p',
    { class: 'pn-input-note' },
    dot(fallback ? 'wait' : bypass !== null ? 'off' : 'on'),
    h(
      'span',
      null,
      fallback ? text.status.upmixFallback(upmix.plugin.name) : text.status.upmix(upmix.plugin.name),
      bypass === null ? null : h('span', { class: 'pn-input-reason' }, bypassReason(bypass)),
    ),
  );
}

/** One input in the column: name, volume, format, a level bar from its loudest object, and what went wrong. */
function inputRow(scope: Scope, lane: number): InputRow {
  const s = text.status;
  const name = text.panel.lamps[lane] ?? String(lane);
  const endpoint = text.endpoints[lane];
  const format = liveText();
  const carrier = liveText();
  const volume = liveText();
  const unsupported = liveText();
  const errorTotal = liveText();
  const epoch = liveText();
  const carrierLine = h('p', { class: 'pn-input-sub', hidden: true }, carrier.node);
  const unsupportedLine = h('p', { class: 'pn-input-note', hidden: true }, dot('wait'), h('span', null, unsupported.node));
  const errorLine = h('p', { class: 'pn-input-note', hidden: true }, dot('fault'), h('span', null, s.chainErrors, ' ', h('span', { class: 'num' }, errorTotal.node)));
  const fill = h('i', { class: 'pn-meter-fill' });
  const element = h(
    'article',
    { class: 'pn-input', 'data-lane': lane, 'data-signal': 'none', 'aria-labelledby': `pn-input-${lane}` },
    h('span', { class: 'pn-input-tick', 'aria-hidden': 'true' }),
    h(
      'div',
      { class: 'pn-input-head' },
      h('h3', { id: `pn-input-${lane}`, class: 'pn-input-name' }, name),
      h('span', { class: 'pn-input-volume num' }, volume.node),
    ),
    endpoint === undefined ? null : h('p', { class: 'pn-input-endpoint' }, endpoint.name),
    h('p', { class: 'pn-input-format' }, format.node),
    carrierLine,
    h('span', { class: 'pn-meter', 'aria-hidden': 'true' }, fill),
    upmixLine(scope.ctx.snapshot.upmix.endpoints.find((view) => view.endpoint === lane)),
    unsupportedLine,
    errorLine,
  );

  const kinds = h('ul', { class: 'pn-kinds' });
  const total = liveText();
  const details = [
    h('div', { class: 'pn-fact' }, h('dt', null, `${name} · ${s.epoch}`), h('dd', { class: 'num', title: s.epochHint }, epoch.node)),
    h('div', { class: 'pn-fact' }, h('dt', null, `${name} · ${s.chainErrors}`), h('dd', null, h('span', { class: 'num' }, total.node), kinds)),
  ];

  let kindsKey = '';
  let lastFill = '';
  return {
    element,
    details,
    update(engine) {
      const status = engine.endpoints[lane];
      if (status === undefined) return;
      let loudest = -Infinity;
      for (const object of engine.scene) {
        if (object.lane === lane && object.level_dbfs > loudest) loudest = object.level_dbfs;
      }
      const sounding = loudest > -Infinity;
      setAttr(element, 'data-signal', sounding ? 'sound' : status.format !== null ? 'idle' : 'none');
      const strength = sounding ? Math.min(1, Math.max(0, (loudest - FLOOR_DB) / -FLOOR_DB)) : 0;
      const scale = `scaleX(${strength.toFixed(3)})`;
      if (scale !== lastFill) {
        fill.style.transform = scale;
        lastFill = scale;
      }
      format.set(describeFormat(status.format));
      const container = describeCarrier(status.format);
      carrierLine.hidden = container === null;
      if (container !== null) carrier.set(container);
      volume.set(describeVolume(status.volume_db, status.muted));
      unsupportedLine.hidden = status.unsupported === null;
      if (status.unsupported !== null) unsupported.set(describeUnsupported(status.unsupported));
      const errors = chainErrorTotal(status.chain_errors);
      errorLine.hidden = errors === 0;
      errorTotal.set(count(errors));
      total.set(errors === 0 ? s.chainErrorsNone : count(errors));
      epoch.set(count(status.epoch));
      const entries = chainErrorEntries(status.chain_errors);
      const key = entries.map((entry) => `${entry.kind}:${entry.count}`).join(',');
      if (key !== kindsKey) {
        kindsKey = key;
        kinds.replaceChildren(
          ...entries.map((entry) => h('li', null, h('span', null, entry.label), h('span', { class: 'num' }, count(entry.count)))),
        );
      }
    },
  };
}

/** The ASIO input while it is on and the engine runs: the DAW, its rate, buffer, layout and underruns, or why not. */
function asioRow(): { element: HTMLElement; update(status: AsioStatus | null): void } {
  const s = text.status;
  const client = liveText();
  const detail = liveText();
  const reason = liveText();
  const detailLine = h('p', { class: 'pn-input-format' }, detail.node);
  const clock = h('p', { class: 'pn-input-note', hidden: true }, dot('wait'), h('span', null, s.asioClockFault));
  const fault = h('p', { class: 'pn-input-note', hidden: true }, dot('fault'), h('span', null, reason.node));
  const element = h(
    'article',
    { class: 'pn-input', 'data-lane': LANES, 'data-signal': 'none', 'aria-labelledby': 'pn-input-asio' },
    h('span', { class: 'pn-input-tick', 'aria-hidden': 'true' }),
    h('div', { class: 'pn-input-head' }, h('h3', { id: 'pn-input-asio', class: 'pn-input-name' }, text.panel.lamps[LANES] ?? text.asio)),
    h('p', { class: 'pn-input-endpoint' }, client.node),
    detailLine,
    clock,
    fault,
  );
  return {
    element,
    update(status) {
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
    },
  };
}

/** A number in the strip: its name, the value large and its unit. */
function metric(
  label: (engine: EngineStatus) => string,
  read: (engine: EngineStatus) => string,
  unit: string,
  warn?: (engine: EngineStatus) => boolean,
) {
  const name = liveText();
  const value = liveText();
  const cell = h(
    'div',
    { class: 'pn-metric' },
    h('dt', null, name.node),
    h('dd', null, h('span', { class: 'num' }, value.node), h('small', null, unit), warn === undefined ? null : dot('wait')),
  );
  return {
    cell,
    update(engine: EngineStatus) {
      name.set(label(engine));
      value.set(read(engine));
      if (warn !== undefined) cell.classList.toggle('is-warning', warn(engine));
    },
  };
}

/** A measurement in the details, as a fact. */
function detail(label: string, unit: string, read: (engine: EngineStatus) => string) {
  const value = liveText();
  return {
    element: h('div', { class: 'pn-fact' }, h('dt', null, label), h('dd', null, h('span', { class: 'num' }, value.node), unit === '' ? null : ` ${unit}`)),
    update: (engine: EngineStatus) => value.set(read(engine)),
  };
}

/**
 * Running. `docked`: the health strip stands at the display's foot, outside
 * the scrolling page (tall windows); otherwise it scrolls with the page.
 */
function running(scope: Scope, engineNow: EngineStatus | null, docked: boolean): PageDrawn {
  const { ctx } = scope;
  const { snapshot } = ctx;
  const m = text.meters;
  const s = text.status;
  const inputs = Array.from({ length: LANES }, (_, lane) => inputRow(scope, lane));
  const asio = (snapshot.asio ?? ASIO_DEFAULTS).enabled ? asioRow() : null;
  const map = soundMap(ctx, engineNow?.directions ?? snapshot.profile.directions, true, 'map-direction');

  const outputFormat = liveText();
  const outputMode = liveText();
  const [profileLabel, profileName] = profileFact(scope);
  const output = section(
    'pn-output',
    s.output,
    [
      kindLine(scope),
      gainNumeral(scope, 'pn-gain-big'),
      facts([
        [profileLabel, profileName],
        [text.panel.device, snapshot.chosen_output?.name ?? s.noOutput],
        [m.output, outputFormat.node],
        [m.outputMode, outputMode.node],
      ]),
      snapshot.roles_taken ? hint(s.rolesTaken) : null,
    ],
    'pn-col pn-output',
  );

  const named = (label: string) => () => label;
  const metrics = [
    metric(named(m.latencyP50), (e) => (e.latency.samples === 0 ? '-' : oneDecimal(e.latency.p50_ms)), m.ms),
    metric(named(m.latencyP99), (e) => (e.latency.samples === 0 ? '-' : oneDecimal(e.latency.p99_ms)), m.ms),
    metric(named(m.ringLevel), (e) => oneDecimal(e.ring_level_ms), m.ms),
    metric(named(m.underruns), (e) => count(e.underruns), m.times, (e) => e.underruns > 0),
    // The clock's kind (trim or ASRC) by its name; the correction large.
    metric((e) => `${m.clock} · ${e.clock === 'trim' ? m.trim : m.asrc}`, (e) => signed(e.clock_ppm), m.ppm),
  ];
  const more = [
    detail(m.outputRestarts, m.times, (e) => count(e.output_restarts)),
    detail(m.decodeRefused, m.times, (e) => count(e.decode_refused)),
    detail(m.lostBytes, m.bytes, (e) => count(e.lost_bytes)),
    detail(m.skippedRecords, m.times, (e) => count(e.skipped_records)),
  ];

  const strip = h('dl', { class: 'pn-metrics', 'aria-label': s.meters }, ...metrics.map((cell) => cell.cell));
  const element = h(
    'div',
    { class: 'pn-status is-running' },
    h(
      'div',
      { class: 'pn-status-main' },
      section('pn-inputs', s.entrances, [...inputs.map((input) => input.element), asio?.element ?? null], 'pn-col pn-inputs'),
      section('pn-map', text.profile.mapTitle, [map.element, hint(text.panel.mapTest)], 'pn-col pn-mapcol'),
      output,
    ),
    docked ? null : strip,
    section('pn-details', text.panel.details, [
      h('dl', { class: 'pn-facts pn-details-grid' }, ...more.map((fact) => fact.element), ...inputs.flatMap((input) => input.details)),
    ]),
  );

  const live: Live = {
    update(engine) {
      for (const input of inputs) input.update(engine);
      asio?.update(engine.asio ?? null);
      map.update(engine);
      outputFormat.set(describeOutputFormat(engine));
      outputMode.set(describeOutputMode(engine));
      for (const cell of metrics) cell.update(engine);
      for (const fact of more) fact.update(engine);
    },
  };
  if (engineNow !== null) live.update(engineNow);
  else asio?.update(null);
  const foot = docked ? { foot: strip } : {};
  return engineNow === null ? { element, ...foot } : { element, live, ...foot };
}

/** Off or stopped: where it will play, the output list while off, the inputs by name, and the power button. */
function standby(scope: Scope): Drawn {
  const { ctx } = scope;
  const { snapshot, view } = ctx;
  const s = text.status;
  const canChange = snapshot.phase.kind === 'ready';
  const chosen = snapshot.chosen_output;
  const [profileLabel, profileName] = profileFact(scope);

  const children: Child[] = [
    h('p', { class: 'pn-standby-label' }, text.panel.standbyDevice),
    h('p', { class: `pn-standby-device${chosen === null ? ' pn-dim' : ''}` }, chosen?.name ?? s.noOutput),
  ];
  if (canChange) {
    children.push(
      row(
        {
          key: 'outputs-toggle',
          label: text.wizard.outputsLabel,
          value: view.outputsOpen ? s.closeOutputs : chosen === null ? s.choose : s.change,
          expanded: view.outputsOpen,
        },
        () => ctx.setView({ outputsOpen: !view.outputsOpen }),
      ),
    );
    if (view.outputsOpen) children.push(h('div', { class: 'pn-sublist' }, outputRows(ctx, 'status-output')));
  }

  const element = h(
    'div',
    { class: 'pn-status is-standby' },
    section('pn-standby', null, children, 'pn-standby'),
    h(
      'div',
      { class: 'pn-standby-grid' },
      section('pn-standby-output', s.output, [
        kindLine(scope),
        gainNumeral(scope, 'pn-gain-standby'),
        facts([[profileLabel, profileName]]),
      ]),
      section(
        'pn-standby-inputs',
        s.entrances,
        text.endpoints.map((endpoint, lane) =>
          h(
            'div',
            { class: 'pn-input is-standby', 'data-lane': lane, 'data-signal': 'none' },
            h('span', { class: 'pn-input-tick', 'aria-hidden': 'true' }),
            h('div', { class: 'pn-input-head' }, h('h3', { class: 'pn-input-name' }, text.panel.lamps[lane] ?? endpoint.name)),
            h('p', { class: 'pn-input-endpoint' }, endpoint.name),
            h('p', { class: 'pn-input-sub' }, endpoint.purpose),
          ),
        ),
      ),
    ),
  );
  return { element };
}

/** A page, and what stands at the display's foot under it. */
export interface PageDrawn extends Drawn {
  foot?: HTMLElement;
}

export function statusPage(scope: Scope, engineNow: EngineStatus | null, docked: boolean): PageDrawn {
  return scope.ctx.snapshot.phase.kind === 'running' ? running(scope, engineNow, docked) : standby(scope);
}
