import { afterEach } from 'vitest';

// happy-dom caches every querySelector result in a WeakRef, and V8 keeps a
// WeakRef's target alive until the job that made it ends. Vitest chains the
// tests of a file through promises alone, so without a task boundary that
// job is the whole file: each found button kept its listeners, the app
// behind them and the screen's tree (about 12 MB a profile screen, 4 GB for
// test/i18n.test.ts). One timer after each test ends the job.
afterEach(() => new Promise<void>((resolve) => setTimeout(resolve, 0)));
