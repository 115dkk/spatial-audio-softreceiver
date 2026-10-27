#!/usr/bin/env node
// Fetches the screen fonts (all OFL-1.1) into
// apps/spatial-softreceiver/ui/public/fonts/<font>/: IBM Plex Sans KR for
// every skin, Galmuri for the panel skin's display, Pretendard for the
// console skin (ADR 0041).
//
// The font files stay out of the repository: this script takes them from the
// npm tarballs, checks each tarball and every file it keeps against the
// SHA-256 values below, and writes nothing that does not match.
// Files already present with the right hash are left alone. Node built-ins
// only, so `npm ci` is not needed to run it.

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync } from 'node:zlib';

/**
 * Each font: the npm tarball it comes from (pinned by SHA-256), the folder in
 * the tarball, the files kept and their SHA-256, and the folder under
 * ui/public/fonts/ they go to. IBM Plex Sans KR is every skin's base; the
 * panel skin's display adds Galmuri and the console skin uses Pretendard
 * (ADR 0041). All three are OFL-1.1.
 */
const FONTS = [
  {
    folder: 'ibm-plex-sans-kr',
    tarball: {
      url: 'https://registry.npmjs.org/@ibm/plex-sans-kr/-/plex-sans-kr-1.1.0.tgz',
      sha256: 'd351afb104446fe1b877144a36f10b3c05ea675de187b573c35a73023c5f9ae5',
    },
    files: {
      'package/fonts/complete/woff2/hinted/IBMPlexSansKR-Regular.woff2':
        '055a35664c3c3965161c92292504c5633ba9604ec904edc1ed799bf2a436276d',
      'package/fonts/complete/woff2/hinted/IBMPlexSansKR-Medium.woff2':
        'c00dcd8a9c32a6b6ab8c6b3119e68b3d7aa6eba6f4441d5493019cc019dcdd1e',
      'package/fonts/complete/woff2/hinted/IBMPlexSansKR-SemiBold.woff2':
        '5ad7db28ba74d59fe14c260205c62ddb701320f4f098d8a45ef2757bebc29666',
      'package/fonts/complete/woff2/hinted/IBMPlexSansKR-Bold.woff2':
        'cf874a368dc2c2c0e4d1933c35611b849e48942023bfdd1fa11128475dbc4860',
      'package/fonts/complete/woff2/hinted/license.txt':
        '91c25c350d3cac39da2736d74f7ba37ef648f5237a4e330a240615bc8d8c4360',
    },
  },
  {
    folder: 'galmuri',
    tarball: {
      url: 'https://registry.npmjs.org/galmuri/-/galmuri-2.40.3.tgz',
      sha256: '7d6b2b8534d157dd5e4eb8e01145f7388c0383bed8a96f5ec719eaacf1f86816',
    },
    files: {
      'package/dist/Galmuri14.woff2': '79b34b22b86536d0fe9ebe7105dd5f20f56558d691c67a0c6432807a03ec5a82',
      'package/dist/Galmuri11.woff2': 'f467d1b10e6b88dfa8399c9f93b38c7643e050abfe4fc15800ac0b000f5a57d6',
      'package/dist/Galmuri11-Bold.woff2': '8643094f395aa2dbad6bc4385cc043314801eaec2e759d549435ec8ac4f2d078',
      'package/dist/LICENSE.txt': '86a3ee9495f942f0243f18c103da9faca27adb88142613edb8bb852e56c892c1',
    },
  },
  {
    folder: 'pretendard',
    tarball: {
      url: 'https://registry.npmjs.org/pretendard/-/pretendard-1.3.9.tgz',
      sha256: '0ffc240cee83be02520fb4ac1b2a0a51052b9d13506ade0ba90c591dbbb3bd53',
    },
    files: {
      'package/dist/web/static/woff2/Pretendard-Medium.woff2':
        'd03481330eeba0659ab5b87f25ceb504a35de377dd90a0d0aba2982eb2d05e2c',
      'package/dist/web/static/woff2/Pretendard-Bold.woff2':
        '4609c3356e536fafe38f4add0daeceb3d8595d3057bce13c428c33ddbd43d362',
      'package/dist/LICENSE.txt': 'b04538c9abec39a3db75108cf0af0fd9c77032fe8aa2cf38345b4d250e98e38e',
    },
  },
];

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const fontsDir = join(root, 'apps', 'spatial-softreceiver', 'ui', 'public', 'fonts');

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

function fail(message) {
  console.error(`ui-fonts: ${message}`);
  process.exit(1);
}

/** The file name a tarball path is written as. */
const base = (path) => path.slice(path.lastIndexOf('/') + 1);

/** Tarball paths whose file is missing or differs; a differing file is removed. */
function stale(font) {
  const target = join(fontsDir, font.folder);
  const paths = [];
  for (const [path, expected] of Object.entries(font.files)) {
    const file = join(target, base(path));
    if (!existsSync(file)) {
      paths.push(path);
    } else if (sha256(readFileSync(file)) !== expected) {
      console.error(`ui-fonts: ${font.folder}/${base(path)} has the wrong SHA-256; removing it`);
      rmSync(file);
      paths.push(path);
    }
  }
  return paths;
}

/** Reads a NUL-terminated ASCII field of a tar header. */
function field(header, start, length) {
  const bytes = header.subarray(start, start + length);
  const end = bytes.indexOf(0);
  return bytes.subarray(0, end === -1 ? length : end).toString('utf8');
}

/**
 * The regular files of a POSIX (ustar) tar archive, by path. Understands the
 * ustar name prefix, pax `path` records and GNU long names, which is all an
 * npm tarball uses.
 */
function untar(archive) {
  const files = new Map();
  let offset = 0;
  let longName = null;
  while (offset + 512 <= archive.length) {
    const header = archive.subarray(offset, offset + 512);
    if (header.every((byte) => byte === 0)) break;
    const size = Number.parseInt(field(header, 124, 12).trim() || '0', 8);
    if (!Number.isFinite(size) || size < 0) throw new Error(`bad size at offset ${offset}`);
    const type = String.fromCharCode(header[156] || 0x30);
    const body = archive.subarray(offset + 512, offset + 512 + size);
    if (body.length !== size) throw new Error('truncated archive');
    offset += 512 + Math.ceil(size / 512) * 512;

    if (type === 'L') {
      longName = field(body, 0, body.length);
      continue;
    }
    if (type === 'x') {
      const match = /(?:^|\n)\d+ path=([^\n]*)\n/.exec(body.toString('utf8'));
      if (match) longName = match[1];
      continue;
    }
    if (type === 'g') continue;

    let name = field(header, 0, 100);
    const prefix = field(header, 345, 155);
    if (field(header, 257, 6).startsWith('ustar') && prefix) name = `${prefix}/${name}`;
    if (longName !== null) {
      name = longName;
      longName = null;
    }
    if (type === '0' || type === '\0' || type === '7') files.set(name, Buffer.from(body));
  }
  return files;
}

/** The body of `url`, with up to three tries for a dropped connection. */
async function download(url) {
  let lastError;
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    try {
      const response = await fetch(url, { redirect: 'follow' });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      return Buffer.from(await response.arrayBuffer());
    } catch (error) {
      lastError = error;
      console.error(`ui-fonts: try ${attempt} failed: ${error instanceof Error ? error.message : error}`);
    }
  }
  throw lastError;
}

async function fetchFont(font) {
  const target = join(fontsDir, font.folder);
  const missing = stale(font);
  if (missing.length === 0) {
    console.log(`ui-fonts: up to date in ${target}`);
    return;
  }

  console.log(`ui-fonts: fetching ${font.tarball.url}`);
  let tarball;
  try {
    tarball = await download(font.tarball.url);
  } catch (error) {
    fail(`download failed: ${error instanceof Error ? error.message : error}`);
  }
  const tarballHash = sha256(tarball);
  if (tarballHash !== font.tarball.sha256) {
    fail(`${font.folder} tarball SHA-256 is ${tarballHash}, expected ${font.tarball.sha256}`);
  }

  const files = untar(gunzipSync(tarball));
  mkdirSync(target, { recursive: true });
  const written = [];
  for (const path of missing) {
    const bytes = files.get(path);
    if (!bytes) {
      for (const file of written) rmSync(file, { force: true });
      fail(`${path} is not in the ${font.folder} tarball`);
    }
    const hash = sha256(bytes);
    if (hash !== font.files[path]) {
      for (const file of written) rmSync(file, { force: true });
      fail(`${font.folder}/${base(path)} SHA-256 is ${hash}, expected ${font.files[path]}`);
    }
    const file = join(target, base(path));
    writeFileSync(file, bytes);
    written.push(file);
    console.log(`ui-fonts: ${font.folder}/${base(path)} ${hash}`);
  }

  const left = stale(font);
  if (left.length > 0) fail(`still wrong after writing: ${left.map(base).join(', ')}`);
}

for (const font of FONTS) await fetchFont(font);
