import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

// Original deterministic score and UI sounds, created for this film.
// Generated audio is dedicated to CC0-1.0; no samples or third-party music.
const directory = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../assets/audio');
fs.mkdirSync(directory, {recursive: true});
const rate = 48000;
const tau = Math.PI * 2;
const hz = midi => 440 * 2 ** ((midi - 69) / 12);
let seed = 0x4e6f7661;
const noise = () => { seed ^= seed << 13; seed ^= seed >>> 17; seed ^= seed << 5; return (seed >>> 0) / 2147483648 - 1; };
function wav(name, seconds, make, peak = 0.65) {
  const length = Math.ceil(seconds * rate);
  const left = new Float32Array(length), right = new Float32Array(length);
  const add = (start, duration, signal, pan = 0) => {
    const offset = Math.round(start * rate);
    const end = Math.min(length, offset + Math.round(duration * rate));
    const gainL = Math.sqrt((1 - pan) / 2), gainR = Math.sqrt((1 + pan) / 2);
    for (let n = Math.max(0, offset); n < end; n++) {
      const value = signal((n - offset) / rate);
      left[n] += value * gainL; right[n] += value * gainR;
    }
  };
  make(add);
  let maximum = 0;
  for (let n = 0; n < length; n++) maximum = Math.max(maximum, Math.abs(left[n]), Math.abs(right[n]));
  const gain = peak / Math.max(maximum, 0.001);
  const data = Buffer.alloc(44 + length * 4);
  data.write('RIFF', 0); data.writeUInt32LE(data.length - 8, 4); data.write('WAVEfmt ', 8);
  data.writeUInt32LE(16, 16); data.writeUInt16LE(1, 20); data.writeUInt16LE(2, 22);
  data.writeUInt32LE(rate, 24); data.writeUInt32LE(rate * 4, 28);
  data.writeUInt16LE(4, 32); data.writeUInt16LE(16, 34); data.write('data', 36); data.writeUInt32LE(length * 4, 40);
  for (let n = 0; n < length; n++) {
    data.writeInt16LE(Math.round(Math.max(-1, Math.min(1, left[n] * gain)) * 32767), 44 + n * 4);
    data.writeInt16LE(Math.round(Math.max(-1, Math.min(1, right[n] * gain)) * 32767), 46 + n * 4);
  }
  fs.writeFileSync(path.join(directory, name), data);
  console.log(`${name}: ${seconds}s, stereo PCM16/${rate}Hz`);
}
const pluck = (frequency, strength = 1) => t => strength * (1 - Math.exp(-t * 100)) * Math.exp(-t * 3.4) *
  (Math.sin(tau * frequency * t) + 0.20 * Math.sin(tau * frequency * 2 * t) + 0.065 * Math.sin(tau * frequency * 3 * t));
const chapters = [0, 8, 22, 39, 54, 68, 82, 98, 109, 116];
wav('nova-original-score.wav', 120, add => {
  // A restrained D / Bm / G / A progression; 120 BPM, no borrowed melody.
  const chords = [[50,57,62,66], [47,54,59,62], [43,50,55,59], [45,52,57,61]];
  for (let bar = 0; bar < 30; bar++) {
    const start = bar * 4, chord = chords[Math.floor(bar / 2) % chords.length];
    for (let voice = 0; voice < chord.length; voice++) {
      const f = hz(chord[voice] + 12), pan = (voice - 1.5) * 0.32;
      add(start, 5, t => {
        const envelope = Math.min(1, t / 0.5) * Math.max(0, 1 - t / 5);
        return 0.026 * envelope * (Math.sin(tau * f * t) + 0.16 * Math.sin(tau * f * 2.003 * t));
      }, pan);
    }
    const pattern = [0,2,1,3,2,1,3,2];
    for (let step = 0; step < 8; step++) {
      const at = start + step * 0.5;
      const energy = at >= 8 && at < 116 ? 1 : 0.72;
      add(at, 2.4, pluck(hz(chord[pattern[step]] + 24), 0.065 * energy), step % 2 ? 0.32 : -0.32);
      if (at >= 8 && at < 116) {
        add(at, 0.22, t => 0.16 * Math.exp(-t * 22) * Math.sin(tau * (48 * t + 15 * (1 - Math.exp(-t * 45)) / 45)));
        if (step % 2) add(at, 0.08, t => 0.022 * noise() * Math.exp(-t * 65), 0.15);
      }
    }
    add(start, 3.5, pluck(hz(chord[0]), 0.12));
  }
  for (const at of chapters.slice(1)) {
    add(at, 1.8, pluck(hz(86), 0.07), -0.2);
    add(at + 0.065, 1.8, pluck(hz(90), 0.055), 0.2);
  }
  // A resolved tail rather than music truncated mid-note.
  add(116, 4, t => 0.08 * Math.sin(tau * hz(74) * t) * Math.exp(-t * 1.7));
}, 0.40);
wav('click.wav', 0.10, add => add(0, 0.08, t => Math.exp(-t * 90) * (Math.sin(tau * 1150 * t) * 0.45 + noise() * 0.16)), 0.32);
wav('swish.wav', 0.40, add => add(0, 0.40, t => noise() * Math.sin(Math.PI * t / 0.40) ** 2 * 0.18, -0.15), 0.19);
wav('confirm.wav', 0.75, add => {add(0, 0.6, pluck(hz(81), 0.2), -0.15); add(0.09, 0.6, pluck(hz(86), 0.2), 0.15);}, 0.35);
wav('soft-impact.wav', 0.5, add => add(0, 0.5, t => Math.exp(-t * 14) * (0.32 * Math.sin(tau * (64 * t + 12 * (1 - Math.exp(-t * 30)) / 30)) + 0.04 * noise())), 0.28);
fs.writeFileSync(path.join(directory, 'LICENSE.txt'), 'Original deterministic Nova film score and UI sound effects.\nCreated by scripts/generate-audio.mjs; no sampled or third-party recordings.\nGenerated audio is dedicated to the public domain under CC0-1.0.\nNo voice-over is present or claimed.\n', 'utf8');
