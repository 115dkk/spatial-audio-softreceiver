// Screen text is a projection of the generated catalog. Sentences from Rust
// (UserError, notes) are shown as they come, not translated by the screen.

import type { Language } from './bindings';
import { en as catalogEn, ko as catalogKo, type Catalog } from './catalog';

type Screen = Catalog['screen'];
type Common = Catalog['common'];
type Supervisor = Catalog['supervisor'];
type Text = Omit<Screen, 'endpoints' | 'chainErrorKinds' | 'status' | 'profile' | 'upmix' | 'settings' | 'entrance'> & {
  product: Catalog['names']['product'];
  endpoints: (Screen['endpoints'][number] & { name: string })[];
  chainErrorKinds: Screen['chainErrorKinds'] & Record<string, string | undefined>;
  status: Screen['status'] & { powerOn: Common['turn_on']; powerOff: Common['turn_off'] };
  profile: Screen['profile'] & {
    fromFile: Common['profile_file_takes_precedence'];
    testOff: Common['test_after_enable'];
    needsBrir: Supervisor['import_brir_first'];
  };
  upmix: Screen['upmix'] & { needsPlugin: Supervisor['choose_plugin_first'] };
  settings: Screen['settings'] & {
    outputPathLocked: Supervisor['disable_before_changing_output'];
    routingUnavailable: Supervisor['entrance_routing_unavailable'];
    asioDevice: Catalog['names']['product'];
  };
  entrance: Screen['entrance'] & {
    osRequirement: Supervisor['app_capture_os_requirement'];
    locked: Supervisor['disable_before_changing_entrance'];
    appsLocked: Supervisor['disable_before_changing_apps'];
    sameApp: Supervisor['capture_same_app'];
    ksNotUsed: Supervisor['ks_not_for_app_capture'];
    noDiscards: Supervisor['no_discard_output'];
  };
};

function project(catalog: Catalog): Text {
  const { screen, names, common, supervisor } = catalog;
  return {
    ...screen,
    product: names.product,
    // Sentences the supervisor also says live once in [common].
    status: { ...screen.status, powerOn: common.turn_on, powerOff: common.turn_off },
    profile: {
      ...screen.profile,
      fromFile: common.profile_file_takes_precedence,
      testOff: common.test_after_enable,
      // The supervisor refuses headphones without a BRIR set in this
      // sentence; the screen says it before the button is pressed.
      needsBrir: supervisor.import_brir_first,
    },
    // A VST upmixer without its plugin: the supervisor's own sentence.
    upmix: { ...screen.upmix, needsPlugin: supervisor.choose_plugin_first },
    settings: {
      ...screen.settings,
      // A running engine refuses set_output_path in this sentence; the
      // screen says it beside the locked choice.
      outputPathLocked: supervisor.disable_before_changing_output,
      // What the supervisor notes when Windows has no per-app output
      // setting; the screen keeps it beside the locked routing controls.
      routingUnavailable: supervisor.entrance_routing_unavailable,
      // The ASIO driver names itself to every DAW in English
      // (apps/ssr-asio, ssr_i18n::names::product(Language::En)), so the
      // screen names that device the way the DAW lists it in both languages.
      asioDevice: catalogEn.names.product,
    },
    // The entrances (ADR 0043): what the supervisor says when it refuses a
    // choice, said beside the control before it is pressed.
    entrance: {
      ...screen.entrance,
      // Why the app-sound entrance cannot be chosen on an older build.
      osRequirement: supervisor.app_capture_os_requirement,
      // A running engine refuses choose_entrance and choose_capture_app in
      // these sentences; the screen says them beside the locked controls.
      locked: supervisor.disable_before_changing_entrance,
      appsLocked: supervisor.disable_before_changing_apps,
      // The app the other lane already captures cannot be chosen twice.
      sameApp: supervisor.capture_same_app,
      // The app-sound entrance does not use kernel streaming.
      ksNotUsed: supervisor.ks_not_for_app_capture,
      // No render device is left to take the apps' original sound.
      noDiscards: supervisor.no_discard_output,
    },
    endpoints: [
      { ...screen.endpoints[0]!, name: names.endpoint_main },
      { ...screen.endpoints[1]!, name: names.endpoint_music },
      { ...screen.endpoints[2]!, name: names.endpoint_pass },
    ],
  };
}

export const ko = project(catalogKo);
export const en = project(catalogEn);

type DeepPartial<T> = T extends (...args: never[]) => unknown
  ? T
  : T extends readonly unknown[]
    ? T
    : T extends object
      ? { [K in keyof T]?: DeepPartial<T[K]> }
      : T;

/**
 * A skin's own words over the catalog's: the same paths with other text
 * (the playground, ADR 0047). `i18n/playground/*.toml` is its source and the
 * catalog test holds every path and placeholder to the catalog's.
 */
export type Overlay = DeepPartial<Pick<Catalog, 'screen' | 'common' | 'names'>>;

function merged<T>(base: T, over: unknown): T {
  if (over === undefined) return base;
  if (typeof base !== 'object' || base === null || Array.isArray(base)) return over as T;
  const out: Record<string, unknown> = { ...(base as Record<string, unknown>) };
  for (const [key, value] of Object.entries(over as Record<string, unknown>)) out[key] = merged(out[key], value);
  return out as T;
}

let language: Language = 'ko';
let overlaid: { ko: Text; en: Text } | null = null;

/** Korean remains the default for the gallery and callers without a snapshot. */
export let text: Text = ko;

function resolve(): void {
  text = (overlaid ?? { ko, en })[language];
}

/** Called with the supervisor's resolved language, never the user's override. */
export function setLanguage(next: Language): void {
  language = next === 'en' ? 'en' : 'ko';
  resolve();
}

/** Lays a skin's words over the catalog for this page; `null` takes them off. */
export function setOverlay(overlay: { ko: Overlay; en: Overlay } | null): void {
  overlaid = overlay === null ? null : { ko: project(merged(catalogKo, overlay.ko)), en: project(merged(catalogEn, overlay.en)) };
  resolve();
}
