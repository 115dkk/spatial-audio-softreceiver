// The 60-second traces (law 3): the contract keeps no history, so the screen
// holds the meters it saw for a minute, a few samples a second. This is view
// state that a reload drops; it starts again empty when the window opens or
// the engine stops, and fewer than two samples draw no line at all.

const WINDOW_MS = 60_000;
/** At most four samples a second, whatever the meters' 30 Hz. */
const SAMPLE_MS = 250;

interface Sample {
  t: number;
  v: number;
}

export class Traces {
  private readonly series = new Map<string, Sample[]>();
  private last = Number.NEGATIVE_INFINITY;

  /**
   * Takes one sample of every value (null: no reading, nothing kept) when
   * the last was long enough ago. True when it took one.
   */
  sample(now: number, values: Readonly<Record<string, number | null>>): boolean {
    if (now - this.last < SAMPLE_MS) return false;
    this.last = now;
    for (const [key, value] of Object.entries(values)) {
      let list = this.series.get(key);
      if (list === undefined) {
        list = [];
        this.series.set(key, list);
      }
      if (value !== null && Number.isFinite(value)) list.push({ t: now, v: value });
      while (list.length > 0 && now - list[0]!.t > WINDOW_MS) list.shift();
    }
    return true;
  }

  clear(): void {
    this.series.clear();
    this.last = Number.NEGATIVE_INFINITY;
  }

  /**
   * The polyline for one series in a `width` × `height` box, the right edge
   * being now and the left 60 seconds ago; null with fewer than two samples.
   */
  points(key: string, now: number, width: number, height: number): string | null {
    const list = this.series.get(key);
    if (list === undefined || list.length < 2) return null;
    let low = Number.POSITIVE_INFINITY;
    let high = Number.NEGATIVE_INFINITY;
    for (const { v } of list) {
      low = Math.min(low, v);
      high = Math.max(high, v);
    }
    const span = high - low;
    const pad = 2;
    const y = (v: number) => (span === 0 ? height / 2 : pad + (1 - (v - low) / span) * (height - pad * 2));
    const x = (t: number) => Math.max(0, width - ((now - t) / WINDOW_MS) * width);
    return list.map(({ t, v }) => `${x(t).toFixed(1)},${y(v).toFixed(1)}`).join(' ');
  }
}
