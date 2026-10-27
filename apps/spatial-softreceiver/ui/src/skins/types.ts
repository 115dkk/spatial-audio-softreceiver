// What a skin is (ADR 0041): a whole renderer of the same contract. A skin
// draws snapshots, sends intents through the Api and keeps only view state,
// like the standard screen (ADR 0001). Its DOM, layout and styles are its own.

import type { Api } from '../api';
import type { EngineStatus, Intent, Snapshot, UserError } from '../bindings';
import type { Tab } from '../screens/common';

export interface App {
  /** Draws a snapshot (the shell's `snapshot` event). */
  draw(snapshot: Snapshot): void;
  /** Updates the meters in place (the shell's `meters` event). */
  updateMeters(engine: EngineStatus): void;
  /** Shows an error that did not come in a snapshot. */
  report(error: UserError): void;
  /** Sends an intent as a button would. */
  send(intent: Intent): Promise<void>;
}

export interface AppOptions {
  /**
   * The screen to open on. A skin shows on its page for a tab everything the
   * standard screen shows on that tab, in its own arrangement.
   */
  tab?: Tab;
}

export interface SkinModule {
  /** Draws into `root` from the next `draw`; only one skin is mounted per page load. */
  mount(root: HTMLElement, api: Api, options?: AppOptions): App;
}
