// Writes the soundtrack, public/audio/music.wav, from code: every sound is
// synthesised here, so the track has no third-party rights attached. It follows
// src/timeline.json bar by bar: the drums drop with the logo, break for the
// formats, drop again for Markup and wind down over the closing.
//
//   node scripts/make-music.mjs

import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import timeline from "../src/timeline.json" with { type: "json" };

const OUT = join(dirname(fileURLToPath(import.meta.url)), "..", "public", "audio", "music.wav");

const SR = 44100;
const BEAT = 60 / timeline.bpm;
const BAR = 4 * BEAT;

// Start bar of every scene, so the arrangement follows the film.
const at = {};
let bar = 0;
for (const [id, length] of Object.entries(timeline.bars)) {
  at[id] = bar;
  bar += length;
}
const END = bar;
const TAIL = 2;
const N = Math.ceil((END * BAR + TAIL) * SR);

const L = new Float32Array(N);
const R = new Float32Array(N);
const sendL = new Float32Array(N);
const sendR = new Float32Array(N);
const echoL = new Float32Array(N);
const echoR = new Float32Array(N);

let seed = 0x2f6b1d;
const random = () => {
  seed = (seed + 0x6d2b79f5) | 0;
  let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
  t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
  return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
const noise = () => random() * 2 - 1;
const mtof = (m) => 440 * 2 ** ((m - 69) / 12);
const sec = (bars) => bars * BAR;
const between = (b, from, to) => b >= from && b < to;

// Topology-preserving state-variable filter.
const svf = () => {
  let ic1 = 0;
  let ic2 = 0;
  return (x, cutoff, q = 0.7) => {
    const g = Math.tan((Math.PI * Math.min(cutoff, SR * 0.45)) / SR);
    const k = 1 / q;
    const a1 = 1 / (1 + g * (g + k));
    const a2 = g * a1;
    const a3 = g * a2;
    const v3 = x - ic2;
    const v1 = a1 * ic1 + a2 * v3;
    const v2 = ic2 + a2 * ic1 + a3 * v3;
    ic1 = 2 * v1 - ic1;
    ic2 = 2 * v2 - ic2;
    return { lp: v2, bp: v1, hp: x - k * v1 - v2 };
  };
};

// Band-limited sawtooth (polyBLEP).
const saw = (phase, dt) => {
  let y = 2 * phase - 1;
  if (phase < dt) {
    const t = phase / dt;
    y -= t + t - t * t - 1;
  } else if (phase > 1 - dt) {
    const t = (phase - 1) / dt;
    y -= t * t + t + t + 1;
  }
  return y;
};

const add = (buf, i, v) => {
  if (i >= 0 && i < N) buf[i] += v;
};

// Sidechain: everything melodic ducks under the kick.
const kicks = [];
const duck = new Float32Array(N).fill(1);

// ---------------------------------------------------------------- harmony

// A minor: Am, F, C, G, one chord per bar.
const chords = [
  [57, 60, 64, 69],
  [53, 57, 60, 65],
  [55, 60, 64, 67],
  [55, 59, 62, 67],
];
const roots = [45, 41, 48, 43];
const chordAt = (b) => chords[Math.floor(b) % 4];
const rootAt = (b) => roots[Math.floor(b) % 4];

// The hook, four bars long: [beat, note, length in beats].
const hook = [
  [0, 76, 0.5], [0.5, 79, 0.5], [1, 81, 1.25], [2.5, 79, 0.5], [3, 76, 1],
  [4, 81, 0.5], [4.5, 79, 0.5], [5, 76, 1.5], [7, 72, 1],
  [8, 76, 0.5], [8.5, 79, 0.5], [9, 84, 1.25], [10.5, 83, 0.5], [11, 79, 1],
  [12, 83, 0.5], [12.5, 81, 0.5], [13, 79, 1], [14, 74, 0.5], [14.5, 76, 1.5],
];

// ---------------------------------------------------------------- arrangement

const drumsOn = (b) =>
  between(b, at.logo, at.formats) || between(b, at.markup, at.closing + 2);
const leadOn = (b) => between(b, at.viewer, at.formats) || between(b, at.markup, at.free);
const impacts = [at.logo, at.markup, at.closing];

// ---------------------------------------------------------------- drums

const kick = (t0, gain = 1) => {
  kicks.push(t0);
  const start = Math.round(t0 * SR);
  let phase = 0;
  for (let i = 0; i < 0.5 * SR; i++) {
    const t = i / SR;
    const f = 44 + 120 * Math.exp(-t * 30);
    phase += f / SR;
    const body = Math.sin(2 * Math.PI * phase) * Math.exp(-t * 6);
    const click = i < 0.004 * SR ? noise() * 0.4 * (1 - i / (0.004 * SR)) : 0;
    const v = Math.tanh(1.8 * body + click) * 0.9 * gain;
    add(L, start + i, v);
    add(R, start + i, v);
  }
};

const clap = (t0, gain = 1) => {
  const start = Math.round(t0 * SR);
  const fl = svf();
  const fr = svf();
  for (let i = 0; i < 0.35 * SR; i++) {
    const t = i / SR;
    const burst = [0, 0.011, 0.022].some((o) => t >= o && t < o + 0.01) ? 1 : 0;
    const env = Math.max(burst * 0.9, t > 0.022 ? Math.exp(-(t - 0.022) * 16) : 0);
    const v = env * 0.4 * gain;
    const l = fl(noise(), 1400, 1.2).bp * v;
    const r = fr(noise(), 1500, 1.2).bp * v;
    add(L, start + i, l);
    add(R, start + i, r);
    add(sendL, start + i, l * 0.3);
    add(sendR, start + i, r * 0.3);
  }
};

const hat = (t0, gain, length = 0.05, pan = 0) => {
  const start = Math.round(t0 * SR);
  const f = svf();
  for (let i = 0; i < length * 4 * SR; i++) {
    const t = i / SR;
    const v = f(noise(), 9000, 0.8).hp * Math.exp(-t / length) * gain;
    add(L, start + i, v * (1 - pan));
    add(R, start + i, v * (1 + pan));
  }
};

const impact = (t0) => {
  const start = Math.round(t0 * SR);
  const f = svf();
  let phase = 0;
  for (let i = 0; i < 2.5 * SR; i++) {
    const t = i / SR;
    phase += (38 + 30 * Math.exp(-t * 8)) / SR;
    const boom = Math.sin(2 * Math.PI * phase) * Math.exp(-t * 2.2) * 0.7;
    const crash = f(noise(), 5000, 0.6).hp * Math.exp(-t * 1.6) * 0.16;
    add(L, start + i, boom + crash * 0.9);
    add(R, start + i, boom + crash * 1.1);
    add(sendL, start + i, crash * 0.6);
    add(sendR, start + i, crash * 0.6);
  }
};

// Filtered noise that rises into a drop.
const riser = (t0, length, gain = 0.25) => {
  const start = Math.round(t0 * SR);
  const fl = svf();
  const fr = svf();
  const n = Math.round(length * SR);
  for (let i = 0; i < n; i++) {
    const p = i / n;
    const cutoff = 300 * (8000 / 300) ** p;
    const v = p ** 2 * gain;
    add(L, start + i, fl(noise(), cutoff, 2.5).bp * v);
    add(R, start + i, fr(noise(), cutoff * 1.05, 2.5).bp * v);
    add(sendL, start + i, fl(noise(), cutoff, 2.5).bp * v * 0.3);
  }
};

for (let b = 0; b < END; b++) {
  const t = sec(b);
  if (drumsOn(b)) {
    const soft = b >= at.closing ? 0.85 : 1;
    for (let k = 0; k < 4; k++) kick(t + k * BEAT, soft);
    clap(t + BEAT, soft);
    clap(t + 3 * BEAT, soft);
    for (let k = 0; k < 4; k++) {
      hat(t + (k + 0.5) * BEAT, 0.22, 0.045, 0.2);
      hat(t + (k + 0.25) * BEAT, 0.06, 0.02, -0.3);
      hat(t + (k + 0.75) * BEAT, 0.07, 0.02, 0.3);
    }
  } else if (between(b, at.formats, at.markup)) {
    for (let k = 0; k < 8; k++) hat(t + k * 0.5 * BEAT, k % 2 ? 0.14 : 0.06, 0.03, 0.15);
  }
}

// A clap roll that speeds up into the second drop.
for (let k = 0; k < 16; k++) {
  if (k < 8 && k % 2) continue;
  clap(sec(at.markup - 1) + k * 0.25 * BEAT, 0.3 + (k / 16) * 0.6);
}

riser(sec(at.logo - 2), sec(2), 0.22);
riser(sec(at.markup - 2), sec(2), 0.28);
riser(sec(at.closing - 1), sec(1), 0.16);
for (const b of impacts) impact(sec(b));

// Sidechain curve: drop to 30 % on the kick, back by 220 ms.
for (const t0 of kicks) {
  const start = Math.round(t0 * SR);
  const n = Math.round(0.22 * SR);
  for (let i = 0; i < n && start + i < N; i++) {
    const p = i / n;
    duck[start + i] = Math.min(duck[start + i], 0.3 + 0.7 * p * p * (3 - 2 * p));
  }
}

// ---------------------------------------------------------------- melodic

// Pad: three detuned saws per note, spread left and right, one chord per bar.
const padCutoff = (b) => {
  if (b < at.logo) return 350 + (b / at.logo) ** 2 * 2600;
  if (between(b, at.formats, at.markup)) return 900 + ((b - at.formats) / 4) ** 2 * 2500;
  if (b >= at.closing) return 2600 - ((b - at.closing) / 4) * 1800;
  return 3200;
};
const padGain = (b) => (b < at.logo ? 0.08 + (b / at.logo) * 0.06 : 0.12);

for (let b = 0; b < END; b++) {
  const start = Math.round(sec(b) * SR);
  const n = Math.round((sec(1) + 0.8) * SR);
  for (const note of chordAt(b)) {
    for (const [cents, pan] of [[-11, -0.8], [0, 0], [12, 0.8]]) {
      const f = mtof(note) * 2 ** (cents / 1200);
      const dt = f / SR;
      let phase = random();
      const filter = svf();
      for (let i = 0; i < n; i++) {
        const t = i / SR;
        const env = Math.min(1, t / 0.12) * (t > sec(1) ? Math.max(0, 1 - (t - sec(1)) / 0.8) : 1);
        const bb = b + t / BAR;
        phase = (phase + dt) % 1;
        const v = filter(saw(phase, dt), padCutoff(bb), 0.8).lp * env * padGain(bb) * 0.33;
        const j = start + i;
        if (j >= N) break;
        const d = drumsOn(bb) ? duck[j] : 1;
        L[j] += v * d * (1 - pan * 0.5);
        R[j] += v * d * (1 + pan * 0.5);
        sendL[j] += v * 0.35;
        sendR[j] += v * 0.35;
      }
    }
  }
}

// Pluck arpeggio in sixteenths, from bar 1, into a ping-pong echo.
const arpOrder = [0, 1, 2, 3, 2, 1, 2, 3];
for (let b = 1; b < END; b++) {
  const chord = chordAt(b);
  for (let s = 0; s < 16; s++) {
    const t0 = sec(b) + s * 0.25 * BEAT;
    const bb = b + s / 16;
    if (bb >= END - 0.5) break;
    const note = chord[arpOrder[s % 8]] + 12;
    const open = b < at.logo ? 800 + (b / at.logo) * 1800 : between(bb, at.formats, at.markup) ? 1800 : 3600;
    const gain = (b < at.logo ? 0.05 : 0.075) * (s % 4 === 0 ? 1 : 0.75);
    const start = Math.round(t0 * SR);
    const f = mtof(note);
    const dt = f / SR;
    let phase = 0;
    const filter = svf();
    for (let i = 0; i < 0.3 * SR; i++) {
      const t = i / SR;
      phase = (phase + dt) % 1;
      const env = Math.exp(-t * 14);
      const v = filter(saw(phase, dt) * 0.6 + (phase < 0.5 ? 0.4 : -0.4), 300 + open * Math.exp(-t * 18), 1.4).lp * env * gain;
      const j = start + i;
      if (j >= N) break;
      const d = drumsOn(bb) ? 0.5 + 0.5 * duck[j] : 1;
      L[j] += v * d;
      R[j] += v * d;
      echoL[j] += v * 0.5;
    }
  }
}

// Bass on the off-beats, with a sub underneath.
for (let b = 0; b < END; b++) {
  if (!drumsOn(b)) continue;
  const root = rootAt(b);
  for (let k = 0; k < 4; k++) {
    const t0 = sec(b) + (k + 0.5) * BEAT;
    const start = Math.round(t0 * SR);
    const f = mtof(root - 12);
    const dt = f / SR;
    let phase = 0;
    const filter = svf();
    const n = Math.round(0.42 * BEAT * SR);
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      phase = (phase + dt) % 1;
      const env = Math.min(1, t / 0.004) * Math.min(1, (n - i) / (0.01 * SR));
      const growl = filter(saw(phase, dt), 180 + 900 * Math.exp(-t * 20), 1.1).lp;
      const v = (growl * 0.5 + Math.sin(2 * Math.PI * phase) * 0.6) * env * 0.32;
      add(L, start + i, v);
      add(R, start + i, v);
    }
  }
}

// Lead: the hook, two saws and a square, a touch of vibrato.
for (let b = 0; b < END; b += 4) {
  if (!leadOn(b)) continue;
  for (const [beat, note, length] of hook) {
    const t0 = sec(b) + beat * BEAT;
    const start = Math.round(t0 * SR);
    const n = Math.round((length * BEAT + 0.15) * SR);
    const f = mtof(note);
    const filter = svf();
    const phases = [0, 0.3, 0.6];
    for (let i = 0; i < n; i++) {
      const t = i / SR;
      const vib = 1 + 0.004 * Math.sin(2 * Math.PI * 5.5 * t) * Math.min(1, t / 0.2);
      const freqs = [f * vib * 0.997, f * vib * 1.003, f * vib];
      let x = 0;
      for (let o = 0; o < 3; o++) {
        const dt = freqs[o] / SR;
        phases[o] = (phases[o] + dt) % 1;
        x += o < 2 ? saw(phases[o], dt) * 0.35 : (phases[o] < 0.5 ? 0.25 : -0.25);
      }
      const held = length * BEAT;
      const env = Math.min(1, t / 0.008) * (t < held ? 0.8 + 0.2 * Math.exp(-t * 8) : Math.max(0, 1 - (t - held) / 0.15));
      const v = filter(x, 1200 + 3200 * Math.exp(-t * 6), 1).lp * env * 0.1;
      const j = start + i;
      if (j >= N) break;
      L[j] += v;
      R[j] += v;
      sendL[j] += v * 0.4;
      sendR[j] += v * 0.4;
      echoR[j] += v * 0.35;
    }
  }
}

// ---------------------------------------------------------------- effects

// Ping-pong echo, a dotted eighth.
{
  const d = Math.round(0.75 * BEAT * SR);
  for (let i = d; i < N; i++) {
    echoL[i] += echoR[i - d] * 0.42;
    echoR[i] += echoL[i - d] * 0.42;
  }
  for (let i = 0; i < N; i++) {
    L[i] += echoL[i] * 0.6;
    R[i] += echoR[i] * 0.6;
  }
}

// Freeverb-style reverb on the send.
const reverb = (input, out, spread) => {
  const combs = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617].map((l) => {
    const len = l + spread;
    return { buf: new Float32Array(len), i: 0, len, store: 0 };
  });
  const allpasses = [556, 441, 341, 225].map((l) => {
    const len = l + spread;
    return { buf: new Float32Array(len), i: 0, len };
  });
  const feedback = 0.86;
  const damp = 0.35;
  for (let n = 0; n < N; n++) {
    const x = input[n] * 0.03;
    let y = 0;
    for (const c of combs) {
      const o = c.buf[c.i];
      c.store = o * (1 - damp) + c.store * damp;
      c.buf[c.i] = x + c.store * feedback;
      c.i = (c.i + 1) % c.len;
      y += o;
    }
    for (const a of allpasses) {
      const o = a.buf[a.i];
      a.buf[a.i] = y + o * 0.5;
      a.i = (a.i + 1) % a.len;
      y = o - y;
    }
    out[n] += y;
  }
};
reverb(sendL, L, 0);
reverb(sendR, R, 23);

// ---------------------------------------------------------------- master

// Fade the tail, soft-clip, then normalise to -1 dBFS.
const fadeFrom = Math.round((END * BAR - 1) * SR);
let peak = 0;
for (let i = 0; i < N; i++) {
  const fade = i > fadeFrom ? Math.max(0, 1 - (i - fadeFrom) / (N - fadeFrom)) : 1;
  L[i] = Math.tanh(L[i] * 1.2) * fade;
  R[i] = Math.tanh(R[i] * 1.2) * fade;
  peak = Math.max(peak, Math.abs(L[i]), Math.abs(R[i]));
}
const gain = 0.89 / peak;

const data = Buffer.alloc(44 + N * 4);
data.write("RIFF", 0);
data.writeUInt32LE(36 + N * 4, 4);
data.write("WAVEfmt ", 8);
data.writeUInt32LE(16, 16);
data.writeUInt16LE(1, 20);
data.writeUInt16LE(2, 22);
data.writeUInt32LE(SR, 24);
data.writeUInt32LE(SR * 4, 28);
data.writeUInt16LE(4, 32);
data.writeUInt16LE(16, 34);
data.write("data", 36);
data.writeUInt32LE(N * 4, 40);
for (let i = 0; i < N; i++) {
  data.writeInt16LE(Math.round(L[i] * gain * 32767), 44 + i * 4);
  data.writeInt16LE(Math.round(R[i] * gain * 32767), 46 + i * 4);
}
mkdirSync(dirname(OUT), { recursive: true });
writeFileSync(OUT, data);
console.log(`music.wav: ${(N / SR).toFixed(1)} s`);
