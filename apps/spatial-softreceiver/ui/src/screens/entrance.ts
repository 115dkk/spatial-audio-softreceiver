// The two ways sound comes in (ADR 0043) as every skin reads them from the
// snapshot: the entrance chosen, whether the app-sound entrance can be chosen
// and why not, the wizard's steps for each, the app-sound lanes with their
// apps and states, and what the lane pickers offer. The supervisor decides
// all of it (ADR 0001); this only names what it says.

import type {
  AppCaptureView,
  CaptureAppChoice,
  CaptureLaneView,
  CaptureState,
  EntranceKind,
  SetupStep,
  Snapshot,
} from '../bindings';
import { text } from '../text';

/** A snapshot from before the entrances means the driver's, as an older settings file does. */
export function entranceOf(snapshot: Snapshot): EntranceKind {
  return snapshot.entrance ?? 'driver';
}

/** The app-sound entrance is the one chosen. */
export function appMode(snapshot: Snapshot): boolean {
  return entranceOf(snapshot) === 'app_sound';
}

const NO_CAPTURE: AppCaptureView = {
  supported: false,
  min_build: 0,
  discard: null,
  discards: [],
  main: null,
  music: null,
  apps: [],
  exclusive: false,
};

/** The app-sound entrance's view; an older snapshot without one has nothing chosen. */
export function captureOf(snapshot: Snapshot): AppCaptureView {
  return snapshot.app_capture ?? NO_CAPTURE;
}

export const ENTRANCE_KINDS: readonly EntranceKind[] = ['driver', 'app_sound'];

export function entranceName(kind: EntranceKind): string {
  return kind === 'driver' ? text.entrance.driver : text.entrance.appSound;
}

/** The control key every skin gives an entrance's choice (ADR 0041's shared handles). */
export function entranceKey(kind: EntranceKind): string {
  return kind === 'driver' ? 'entrance-driver' : 'entrance-app-sound';
}

/** What the entrance receives, in one sentence. */
export function entranceGets(kind: EntranceKind): string {
  return kind === 'driver' ? text.entrance.driverGets : text.entrance.appSoundGets;
}

/** What the entrance needs, a sentence each; the build only once the supervisor has given it. */
export function entranceNeeds(kind: EntranceKind, snapshot: Snapshot): string[] {
  const e = text.entrance;
  if (kind === 'driver') return [e.driverNeeds];
  const build = captureOf(snapshot).min_build;
  return build > 0 ? [e.appSoundBuild(build), e.appSoundDiscard] : [e.appSoundDiscard];
}

/**
 * Whether choosing the app-sound entrance can work here, and the
 * supervisor's own sentence when it cannot: the supervisor read a Windows
 * build older than the one it needs. Before the PC is read (the welcome
 * step) nothing is known, so the choice stays open and the supervisor
 * answers it.
 */
export function appSoundBlocked(snapshot: Snapshot): string | null {
  const view = captureOf(snapshot);
  if (view.supported || snapshot.platform === null || view.min_build <= 0) return null;
  return text.entrance.osRequirement(view.min_build);
}

/** Why an entrance cannot be chosen now: the engine is running, or the PC cannot run it. */
export function entranceBlocked(snapshot: Snapshot, kind: EntranceKind): string | null {
  if (snapshot.phase.kind === 'running') return text.entrance.locked;
  return kind === 'app_sound' ? appSoundBlocked(snapshot) : null;
}

/** The wizard's step names for the entrance chosen. */
export function stepLabels(entrance: EntranceKind): readonly string[] {
  return entrance === 'app_sound' ? text.wizard.appSteps : text.wizard.steps;
}

/**
 * Which step of `stepLabels(entrance)` a setup step belongs to. The driver
 * entrance: welcome, check, driver, output. The app-sound entrance:
 * entrance (the welcome step and the build check), output, discard output,
 * ready.
 */
export function stepIndex(step: SetupStep, entrance: EntranceKind = 'driver'): number {
  if (entrance === 'app_sound') {
    switch (step) {
      case 'welcome':
      case 'unsupported':
        return 0;
      case 'choose_output':
        return 1;
      case 'choose_discard':
        return 2;
      default:
        break;
    }
  }
  switch (step) {
    case 'welcome':
      return 0;
    case 'unsupported':
    case 'not_test_signing':
      return 1;
    case 'driver':
    case 'reboot_required':
      return 2;
    case 'choose_output':
    case 'choose_discard':
      return 3;
  }
}

/** The app-sound lanes: main (0) and music (1). PASS (2) is the driver's alone. */
export const CAPTURE_LANES: readonly number[] = [0, 1];
export const PASS_LANE = 2;

export function laneName(lane: number): string {
  return text.entrance.lanes[lane] ?? String(lane);
}

export function laneView(snapshot: Snapshot, lane: number): CaptureLaneView | null {
  const view = captureOf(snapshot);
  return lane === 0 ? view.main : lane === 1 ? view.music : null;
}

export function captureStateWord(state: CaptureState): string {
  const e = text.entrance;
  switch (state) {
    case 'not_running':
      return e.stateNotRunning;
    case 'running':
      return e.stateRunning;
    case 'capturing':
      return e.stateCapturing;
    case 'waiting':
      return e.stateWaiting;
  }
}

/** How a lane's state stands: taking sound, waiting or missing, or only ready. */
export function captureTone(state: CaptureState): 'on' | 'off' | 'warning' {
  return state === 'capturing' ? 'on' : state === 'running' ? 'off' : 'warning';
}

/** An app as a picker names it: its name, and the program file when that says more. */
export function appLabel(app: { exe: string; display_name: string }): string {
  return app.display_name === '' || app.display_name === app.exe ? app.exe : `${app.display_name} (${app.exe})`;
}

export interface PickerOption {
  /** The program file, or null for the music lane's "none". */
  exe: string | null;
  label: string;
  /** Why it cannot be chosen: the other lane captures it. */
  blocked: string | null;
}

/**
 * What a lane's picker offers: "none" on the music lane (the main lane is
 * needed to turn on), the lane's own app when it is not running now, and the
 * apps with a sound session. The other lane's app is offered with the reason
 * the supervisor would refuse it.
 */
export function pickerOptions(snapshot: Snapshot, lane: number): PickerOption[] {
  const view = captureOf(snapshot);
  const own = laneView(snapshot, lane);
  const other = laneView(snapshot, lane === 0 ? 1 : 0);
  const options: PickerOption[] = lane === 0 ? [] : [{ exe: null, label: text.profile.none, blocked: null }];
  if (own !== null && !view.apps.some((app) => app.exe === own.exe)) {
    options.push({ exe: own.exe, label: appLabel(own), blocked: null });
  }
  for (const app of view.apps) {
    options.push({ exe: app.exe, label: appLabel(app), blocked: other?.exe === app.exe ? text.entrance.sameApp : null });
  }
  return options;
}

/** The control key every skin gives a lane's picker. */
export const pickerKey = (lane: number): string => `capture-app-${lane}`;

/** Why the pickers cannot be used now: the engine is running. */
export function pickersLocked(snapshot: Snapshot): string | null {
  return snapshot.phase.kind === 'running' ? text.entrance.appsLocked : null;
}

/** The intent a picker sends. */
export function chooseApp(lane: number, exe: string | null): { kind: 'choose_capture_app'; value: CaptureAppChoice } {
  return { kind: 'choose_capture_app', value: { lane, exe } };
}
