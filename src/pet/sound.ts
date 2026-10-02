// Glowby's sounds, synthesised with WebAudio: no sound files.
//
// Each sound is a few oscillators (sine/triangle waves) with a quick fade-in
// and a soft fade-out ("envelope"). After the sound, the audio device is
// suspended again, so Glowby doesn't keep an audio stream running while idle.

let ctx: AudioContext | null = null;
let suspendTimer = 0;

function audio(): AudioContext {
  ctx ??= new AudioContext();
  if (ctx.state === "suspended") void ctx.resume();
  return ctx;
}

/** One soft note. `glideTo` bends the pitch (for yawns and "oh no" sounds). */
function tone(
  c: AudioContext,
  out: AudioNode,
  freq: number,
  start: number,
  duration: number,
  type: OscillatorType = "sine",
  level = 0.5,
  glideTo?: number,
) {
  const osc = c.createOscillator();
  const gain = c.createGain();
  osc.type = type;
  osc.frequency.setValueAtTime(freq, start);
  if (glideTo) osc.frequency.exponentialRampToValueAtTime(glideTo, start + duration);
  gain.gain.setValueAtTime(0.0001, start);
  gain.gain.exponentialRampToValueAtTime(level, start + 0.015);
  gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
  osc.connect(gain).connect(out);
  osc.start(start);
  osc.stop(start + duration + 0.05);
}

/** Plays a named sound at volume 0..1. Returns how long it lasts (seconds). */
export function playSound(name: string, volume: number): number {
  if (volume <= 0) return 0;
  const c = audio();
  const out = c.createGain();
  out.gain.value = Math.min(1, volume) * 0.35; // keep it gentle
  out.connect(c.destination);
  const t = c.currentTime + 0.03;
  let length = 0.4;
  switch (name) {
    case "done": // two rising notes
      tone(c, out, 659, t, 0.22);
      tone(c, out, 880, t + 0.11, 0.32);
      length = 0.45;
      break;
    case "alert": // two quick blips
      tone(c, out, 988, t, 0.1, "triangle", 0.4);
      tone(c, out, 988, t + 0.16, 0.1, "triangle", 0.4);
      length = 0.3;
      break;
    case "notice":
      tone(c, out, 784, t, 0.25, "sine", 0.45);
      break;
    case "error": // a soft "oh no", sliding down
      tone(c, out, 392, t, 0.38, "triangle", 0.45, 262);
      length = 0.45;
      break;
    case "levelup": // happy arpeggio
      [523, 659, 784, 1047].forEach((f, i) => tone(c, out, f, t + i * 0.09, 0.26, "sine", 0.5));
      length = 0.65;
      break;
    case "evolve": // two octaves up with a shimmer on top
      [523, 659, 784, 1047, 1319, 1568].forEach((f, i) => tone(c, out, f, t + i * 0.08, 0.3, "sine", 0.45));
      tone(c, out, 2093, t + 0.5, 0.6, "sine", 0.18);
      length = 1.15;
      break;
    case "break": // a little yawn
      tone(c, out, 620, t, 0.55, "sine", 0.4, 420);
      tone(c, out, 520, t + 0.45, 0.5, "sine", 0.3, 360);
      length = 1.0;
      break;
    case "pet": // a soft purr and a happy chirp
      for (let i = 0; i < 5; i++) tone(c, out, 110 + (i % 2) * 8, t + i * 0.085, 0.09, "triangle", 0.35);
      tone(c, out, 880, t + 0.46, 0.14, "sine", 0.3, 1175);
      length = 0.65;
      break;
    case "boop": // emote
      tone(c, out, 700, t, 0.12, "sine", 0.4, 920);
      length = 0.2;
      break;
    default:
      return 0;
  }
  window.clearTimeout(suspendTimer);
  suspendTimer = window.setTimeout(() => void ctx?.suspend(), (length + 1) * 1000);
  return length;
}
