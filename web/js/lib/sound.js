/** Synthesized alert tones via Web Audio API. No audio files. */

import * as prefs from "./prefs.js";

const SOUNDS = {
  blocked: [
    { hz: 587.33, at: 0, len: 0.16 },
    { hz: 440.0, at: 0.14, len: 0.24 },
  ],
  ready: [
    { hz: 523.25, at: 0, len: 0.12 },
    { hz: 783.99, at: 0.1, len: 0.22 },
  ],
  remind: [
    { hz: 440.0, at: 0, len: 0.1 },
    { hz: 440.0, at: 0.22, len: 0.1 },
  ],
};

/** @type {AudioContext | null} */
let ctx = null;
let unlocked = false;

function alertsPrefs() {
  return (
    prefs.get("alerts", {
      sound: true,
      volume: 0.7,
      notify: true,
      remind_minutes: 5,
      on_ready: true,
    }) || {
      sound: true,
      volume: 0.7,
      notify: true,
      remind_minutes: 5,
      on_ready: true,
    }
  );
}

function ensureContext() {
  if (typeof window === "undefined") return null;
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) return null;
  if (!ctx) ctx = new AC();
  return ctx;
}

/** Call from a user gesture so later tones can play. */
export function unlock() {
  const c = ensureContext();
  if (!c) return;
  unlocked = true;
  if (c.state === "suspended") {
    c.resume().catch(() => {});
  }
}

export function play(name) {
  const settings = alertsPrefs();
  if (!settings.sound) return;
  const volume = Number(settings.volume);
  if (!(volume > 0)) return;
  if (!unlocked) return;

  const notes = SOUNDS[name];
  if (!notes) return;
  const c = ensureContext();
  if (!c) return;
  if (c.state === "suspended") {
    c.resume().catch(() => {});
  }

  const peak = 0.12 * Math.min(1, Math.max(0, volume));
  const t0 = c.currentTime;

  for (const note of notes) {
    const osc = c.createOscillator();
    const gain = c.createGain();
    osc.type = "sine";
    osc.frequency.value = note.hz;
    const start = t0 + note.at;
    const end = start + note.len;
    gain.gain.setValueAtTime(0.0001, start);
    gain.gain.exponentialRampToValueAtTime(peak, start + 0.01);
    gain.gain.exponentialRampToValueAtTime(0.0001, end);
    osc.connect(gain);
    gain.connect(c.destination);
    osc.start(start);
    osc.stop(end + 0.02);
  }
}

export function playTest() {
  unlock();
  play("blocked");
}
