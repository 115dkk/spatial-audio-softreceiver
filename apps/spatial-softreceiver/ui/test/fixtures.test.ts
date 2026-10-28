import { describe, expect, it } from 'vitest';
import intents from '../fixtures/intents.json';
import { fixture, fixtureNames, rawFixture } from '../src/fixtures';
import { ASIO_DEFAULTS, ASIO_LAYOUTS, ShapeError, assertIntent, assertSnapshot } from '../src/guard';
import { createMockApi } from '../src/mock';

describe('fixtures match bindings.ts', () => {
  it('lists every fixture file', () => {
    expect(fixtureNames.length).toBeGreaterThan(0);
    for (const name of fixtureNames) expect(() => rawFixture(name)).not.toThrow();
  });

  it.each(fixtureNames)('%s passes assertSnapshot', (name) => {
    const raw = rawFixture(name);
    expect(assertSnapshot(raw)).toEqual(raw);
  });

  it('has the Korean fixtures in Korean', () => {
    for (const name of fixtureNames) expect(fixture(name).language, name).toBe('ko');
  });

  it('covers every intent', () => {
    const kinds = intents.map((intent) => assertIntent(intent).kind);
    expect(new Set(kinds)).toEqual(
      new Set([
        'set_language',
        'set_skin',
        'set_output_path',
        'set_routing_enabled',
        'set_app_rule',
        'remove_app_rule',
        'begin_setup',
        'install_driver',
        'recheck',
        'choose_output',
        'set_profile_kind',
        'import_brir',
        'set_speaker_layout',
        'import_correction',
        'clear_correction',
        'set_gain',
        'test_direction',
        'set_upmixer',
        'choose_plugin',
        'open_plugin_editor',
        'set_upmix_apply',
        'set_asio_enabled',
        'set_asio_layout',
        'set_asio_system_attenuation',
        'enable',
        'disable',
        'start_diagnostic_recording',
        'quit',
      ]),
    );
  });
});

describe('ASIO contract wiring', () => {
  /** The fixtures of the ASIO input's own states; every other one keeps the defaults and no client. */
  const asioFixtures = ['running-asio', 'running-asio-clock-fault', 'running-asio-fault', 'ready-asio-off', 'ready-asio-24-1-10'];

  it('keeps ASIO settings in every snapshot without inventing a client', () => {
    expect(asioFixtures.filter((name) => !fixtureNames.includes(name)), 'ASIO fixtures Rust no longer writes').toEqual([]);
    for (const language of ['ko', 'en'] as const) {
      for (const name of fixtureNames.filter((candidate) => !asioFixtures.includes(candidate))) {
        const snapshot = fixture(name, language);
        expect(snapshot.asio, name).toEqual(ASIO_DEFAULTS);
        if (snapshot.engine !== null) expect(snapshot.engine.asio, name).toBeNull();
      }
    }
  });

  it.each(['ko', 'en'] as const)('has a fixture for each state of the ASIO input (%s)', (language) => {
    const attached = fixture('running-asio', language);
    expect(attached.asio).toEqual({ enabled: true, layout: '7.1.4', system_attenuation_db: 12 });
    expect(attached.engine?.asio).toMatchObject({ client_exe: 'reaper.exe', layout: '7.1.4', channels: 12, clock_fault: false, fault_reason: null });
    expect(fixture('running-asio-clock-fault', language).engine?.asio).toMatchObject({ clock_fault: true, fault_reason: null });
    const failed = fixture('running-asio-fault', language);
    expect(failed.engine?.asio).toMatchObject({ client_exe: '', fault_reason: 'ASIO transport failed: owner_exists' });
    expect(failed.notes).toHaveLength(1);
    expect(fixture('ready-asio-off', language).asio?.enabled).toBe(false);
    // A layout of more than 32 channels (ADR 0042), with no DAW attached.
    const high = fixture('ready-asio-24-1-10', language);
    expect(high.asio).toEqual({ enabled: true, layout: '24.1.10', system_attenuation_db: 0 });
    expect(high.engine).toBeNull();
  });

  it('accepts the eleven layouts in the contract order and the complete client status', () => {
    // The five older layouts first; ADR 0042 appended the rest after them.
    expect(Object.keys(ASIO_LAYOUTS)).toEqual(['2.0', '5.1', '7.1', '7.1.4', '9.1', '9.1.4', '9.1.6', '13.1', '22.2', '24.1.10', '30.2']);
    for (const layout of Object.keys(ASIO_LAYOUTS)) {
      expect(assertIntent({ kind: 'set_asio_layout', value: layout })).toEqual({ kind: 'set_asio_layout', value: layout });
      const value = structuredClone(rawFixture('running')) as { engine: Record<string, unknown> };
      value.engine.asio = {
        client_pid: 42, client_exe: 'test-daw.exe', sample_rate: 48000, buffer_frames: 256,
        channels: 12, layout, underruns: 2, generation: 3, clock_fault: true, fault_reason: 'owner_exists',
      };
      expect(assertSnapshot(value).engine?.asio?.layout).toBe(layout);
      delete (value.engine.asio as Record<string, unknown>).client_pid;
      expect(() => assertSnapshot(value)).toThrow('snapshot.engine.asio.client_pid');
    }
    expect(() => assertIntent({ kind: 'set_asio_layout', value: '5.1.4' })).toThrow('intent.value');
    expect(() => assertIntent({ kind: 'set_asio_enabled', value: 1 })).toThrow('intent.value');
    expect(() => assertIntent({ kind: 'set_asio_system_attenuation', value: Number.NaN })).toThrow('intent.value');
  });

  it('mock settings dispatch preserves the absent client and rejects invalid attenuation', async () => {
    const api = createMockApi(fixture('running'), { meters: false });
    await api.dispatch({ kind: 'set_asio_enabled', value: false });
    await api.dispatch({ kind: 'set_asio_layout', value: '9.1.6' });
    await api.dispatch({ kind: 'set_asio_system_attenuation', value: 60 });
    const expected = { enabled: false, layout: '9.1.6', system_attenuation_db: 60 };
    expect((await api.snapshot()).asio).toEqual(expected);
    expect((await api.snapshot()).engine?.asio).toBeNull();
    for (const value of [-1, 61, Number.NaN, Number.POSITIVE_INFINITY]) {
      await expect(api.dispatch({ kind: 'set_asio_system_attenuation', value })).rejects.toBeDefined();
      expect((await api.snapshot()).asio).toEqual(expected);
    }
  });
});

describe('the English fixtures', () => {
  const englishFiles = Object.keys(import.meta.glob('../fixtures/en/*.json'))
    .map((path) => path.replace(/^.*\//, '').replace(/\.json$/, ''))
    .sort();

  it('are the same states as the Korean ones', () => {
    expect(englishFiles).toEqual([...fixtureNames].sort());
  });

  it.each(fixtureNames)('%s passes assertSnapshot', (name) => {
    const raw = rawFixture(name, 'en');
    expect(assertSnapshot(raw)).toEqual(raw);
  });

  it.each(fixtureNames)('%s is in English and in the same state as the Korean one', (name) => {
    const english = fixture(name, 'en');
    const korean = fixture(name);
    expect(english.language).toBe('en');
    expect(english.language_choice).toEqual(korean.language_choice);
    expect(english.phase).toEqual(korean.phase);
    expect(english.error?.next ?? null).toEqual(korean.error?.next ?? null);
    expect(english.notes).toHaveLength(korean.notes.length);
    expect(english.engine === null).toBe(korean.engine === null);
    expect(english.profile.kind).toBe(korean.profile.kind);
  });

  it('a fresh copy every call, apart from the Korean one', () => {
    const first = fixture('running', 'en');
    first.notes.push('changed');
    expect(fixture('running', 'en').notes).not.toContain('changed');
    expect(fixture('running', 'en')).not.toEqual(fixture('running'));
  });

  it('names a missing fixture with its language', () => {
    expect(() => rawFixture('no-such-state', 'en')).toThrow('no en fixture named no-such-state');
  });
});

describe('the guards reject what bindings.ts does not allow', () => {
  const running = () => structuredClone(rawFixture('running')) as Record<string, unknown> & {
    phase: { kind: string };
    engine: Record<string, unknown>;
  };

  it('an unknown phase kind', () => {
    const value = running();
    value.phase.kind = 'paused';
    expect(() => assertSnapshot(value)).toThrow(ShapeError);
  });

  it('a missing field, with its path', () => {
    const value = running();
    delete value.engine.decode_refused;
    expect(() => assertSnapshot(value)).toThrow('snapshot.engine.decode_refused');
  });

  it('a field bindings.ts does not have', () => {
    const value = running();
    value.volume = 3;
    expect(() => assertSnapshot(value)).toThrow('snapshot.volume');
  });

  it('a wrong unit enum name', () => {
    const value = running();
    value.engine.clock = 'pll';
    expect(() => assertSnapshot(value)).toThrow('snapshot.engine.clock');
  });

  it('a codec by name or by number', () => {
    const value = running();
    const endpoints = value.engine.endpoints as { format: { value: { codec: unknown } } }[];
    const slot = endpoints[1];
    if (slot === undefined) throw new Error('fixture has no endpoint 1');
    slot.format.value.codec = { other: 0x1234 };
    expect(() => assertSnapshot(value)).not.toThrow();
    slot.format.value.codec = 'atmos';
    expect(() => assertSnapshot(value)).toThrow('codec');
  });

  it('an intent with the wrong value', () => {
    expect(() => assertIntent({ kind: 'set_gain', value: '-3' })).toThrow('intent.value');
    expect(() => assertIntent({ kind: 'enable', value: 1 })).toThrow('intent.value');
    expect(() => assertIntent({ kind: 'set_upmixer', value: { endpoint: 1, choice: 'vst' } })).toThrow('intent.value.choice');
    expect(() => assertIntent({ kind: 'choose_plugin', value: { endpoint: 1 } })).toThrow('intent.value.path');
    expect(() => assertIntent({ kind: 'open_plugin_editor', value: '1' })).toThrow('intent.value');
    expect(() => assertIntent({ kind: 'set_upmix_apply', value: { kind: 'stereo', on: true } })).toThrow('intent.value.kind');
    // The choice of how stereo arrives is gone (ADR 0035, the 2026-10-16 correction).
    expect(() => assertIntent({ kind: 'set_upmix_sources', value: { endpoint: 1, sources: { pcm: true, bitstream: true } } })).toThrow(
      'intent.kind',
    );
    expect(() => assertIntent({ kind: 'set_upmixer', value: { endpoint: '2', choice: 'plugin' } })).toThrow('intent.value.endpoint');
    expect(assertIntent({ kind: 'set_upmixer', value: { endpoint: 2, choice: 'plugin' } })).toEqual({
      kind: 'set_upmixer',
      value: { endpoint: 2, choice: 'plugin' },
    });
  });

  it('an upmix state, plugin format or choice bindings.ts does not have', () => {
    const music = () => {
      const value = structuredClone(rawFixture('running-upmix')) as {
        upmix: { endpoints: { state: unknown; plugin: { format: unknown }; effective: unknown }[] };
      };
      const endpoint = value.upmix.endpoints[1];
      if (endpoint === undefined) throw new Error('running-upmix has a music endpoint');
      return { value, endpoint };
    };
    let { value, endpoint } = music();
    endpoint.state = { kind: 'paused' };
    expect(() => assertSnapshot(value)).toThrow('snapshot.upmix.endpoints[1].state.kind');
    ({ value, endpoint } = music());
    endpoint.state = { kind: 'failed' };
    expect(() => assertSnapshot(value)).toThrow('snapshot.upmix.endpoints[1].state.value');
    ({ value, endpoint } = music());
    endpoint.plugin.format = 'au';
    expect(() => assertSnapshot(value)).toThrow('snapshot.upmix.endpoints[1].plugin.format');
    ({ value, endpoint } = music());
    endpoint.effective = null;
    expect(() => assertSnapshot(value)).toThrow('snapshot.upmix.endpoints[1].effective');
  });

  it('a bypass, a choice of how stereo arrives or an input list bindings.ts does not have', () => {
    const pass = () => {
      const value = structuredClone(rawFixture('running-upmix-native-surround')) as {
        upmix: { endpoints: Record<string, unknown>[] };
      };
      const endpoint = value.upmix.endpoints[2];
      if (endpoint === undefined) throw new Error('running-upmix-native-surround has a PASS endpoint');
      return { value, endpoint };
    };
    expect(assertSnapshot(pass().value).upmix.endpoints.map((endpoint) => endpoint.endpoint)).toEqual([0, 1, 2]);
    expect(assertSnapshot(pass().value).upmix.endpoints[2]?.bypass).toBe('native_surround');
    for (const bypass of ['kind_off', 'rate_change', null]) {
      const { value, endpoint } = pass();
      endpoint.bypass = bypass;
      expect(assertSnapshot(value).upmix.endpoints[2]?.bypass).toBe(bypass);
    }
    for (const bypass of ['source_off', 'multichannel']) {
      const { value, endpoint } = pass();
      endpoint.bypass = bypass;
      expect(() => assertSnapshot(value), bypass).toThrow('snapshot.upmix.endpoints[2].bypass');
    }
    let { value, endpoint } = pass();
    delete endpoint.bypass;
    expect(() => assertSnapshot(value)).toThrow('snapshot.upmix.endpoints[2].bypass');
    ({ value, endpoint } = pass());
    endpoint.sources = { pcm: true, bitstream: true };
    expect(() => assertSnapshot(value)).toThrow('snapshot.upmix.endpoints[2].sources');
    ({ value, endpoint } = pass());
    endpoint.inputs = 'L R';
    expect(() => assertSnapshot(value)).toThrow('snapshot.upmix.endpoints[2].inputs');
    ({ value, endpoint } = pass());
    endpoint.inputs = ['L', 2];
    expect(() => assertSnapshot(value)).toThrow('snapshot.upmix.endpoints[2].inputs[1]');
  });
});
