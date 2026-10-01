// Glowby's colours and "body language" for each mood.
// The renderer blends between these smoothly when the mood changes.

export type Mood = "idle" | "working" | "happy" | "alert" | "sleepy" | "sick";
export type RGB = [number, number, number];

export interface MoodStyle {
  bell: RGB;
  edge: RGB;
  inner: RGB;
  tentacle: RGB;
  halo: RGB;
  haloAlpha: number;
  /** Bell pulses per second (a jellyfish "swims" by pulsing). */
  pulseHz: number;
  pulseAmp: number;
  /** How much the tentacles wave. */
  wave: number;
}

const hex = (h: string): RGB => [
  parseInt(h.slice(1, 3), 16),
  parseInt(h.slice(3, 5), 16),
  parseInt(h.slice(5, 7), 16),
];

const calm = {
  bell: hex("#AFC2FF"),
  edge: hex("#5C77D6"),
  inner: hex("#DCE4FF"),
  tentacle: hex("#E08CC6"),
  halo: hex("#85A0FF"),
};

export const STYLES: Record<Mood, MoodStyle> = {
  idle: { ...calm, haloAlpha: 0.18, pulseHz: 0.35, pulseAmp: 1, wave: 1 },
  working: { ...calm, haloAlpha: 0.36, pulseHz: 0.95, pulseAmp: 1.3, wave: 1.4 },
  happy: { ...calm, bell: hex("#B9CBFF"), haloAlpha: 0.28, pulseHz: 0.6, pulseAmp: 1.1, wave: 1.2 },
  alert: {
    bell: hex("#FFD08A"),
    edge: hex("#C9851F"),
    inner: hex("#FFEBC8"),
    tentacle: hex("#EF9F27"),
    halo: hex("#EF9F27"),
    haloAlpha: 0.3,
    pulseHz: 1.3,
    pulseAmp: 1.2,
    wave: 1.1,
  },
  sleepy: {
    bell: hex("#BAC4E6"),
    edge: hex("#7D88B5"),
    inner: hex("#DADFF0"),
    tentacle: hex("#C9A5BE"),
    halo: hex("#8F9CC8"),
    haloAlpha: 0.07,
    pulseHz: 0.18,
    pulseAmp: 0.6,
    wave: 0.45,
  },
  sick: {
    bell: hex("#C9E2B3"),
    edge: hex("#7E9F5E"),
    inner: hex("#E3F0D7"),
    tentacle: hex("#A7C47F"),
    halo: hex("#9FCB7A"),
    haloAlpha: 0.12,
    pulseHz: 0.3,
    pulseAmp: 0.7,
    wave: 0.6,
  },
};

const mixRGB = (a: RGB, b: RGB, t: number): RGB => [
  a[0] + (b[0] - a[0]) * t,
  a[1] + (b[1] - a[1]) * t,
  a[2] + (b[2] - a[2]) * t,
];
const mixNum = (a: number, b: number, t: number) => a + (b - a) * t;

/** Moves style `a` a fraction `t` of the way towards style `b`. */
export function blendStyle(a: MoodStyle, b: MoodStyle, t: number): MoodStyle {
  return {
    bell: mixRGB(a.bell, b.bell, t),
    edge: mixRGB(a.edge, b.edge, t),
    inner: mixRGB(a.inner, b.inner, t),
    tentacle: mixRGB(a.tentacle, b.tentacle, t),
    halo: mixRGB(a.halo, b.halo, t),
    haloAlpha: mixNum(a.haloAlpha, b.haloAlpha, t),
    pulseHz: mixNum(a.pulseHz, b.pulseHz, t),
    pulseAmp: mixNum(a.pulseAmp, b.pulseAmp, t),
    wave: mixNum(a.wave, b.wave, t),
  };
}

export const rgba = (c: RGB, a = 1) => `rgba(${c[0] | 0},${c[1] | 0},${c[2] | 0},${a})`;

export const INK = "#24263A";
