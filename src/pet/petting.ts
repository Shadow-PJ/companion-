// Petting: stroking the mouse back and forth over a pet, without pressing a
// button (pressing means dragging or clicking). Two direction changes within
// 1.4 s, each after the hand moved at least a few pixels, count as petting;
// every further change keeps it going.

const MIN_TRAVEL_PX = 6;
const WINDOW_MS = 1400;
const TURNS_NEEDED = 2;

export function strokeDetector(onStroke: (x: number, y: number) => void) {
  let lastX: number | null = null;
  let dir = 0;
  let travel = 0;
  let turns: number[] = [];
  return (e: Pick<PointerEvent, "buttons" | "clientX" | "clientY" | "timeStamp">) => {
    if (e.buttons) {
      lastX = null;
      turns = [];
      return;
    }
    if (lastX === null) {
      lastX = e.clientX;
      return;
    }
    const dx = e.clientX - lastX;
    lastX = e.clientX;
    if (Math.abs(dx) < 0.5) return;
    const d = Math.sign(dx);
    if (d === dir) {
      travel += Math.abs(dx);
      return;
    }
    // changed direction: it counts if the hand travelled a bit first
    const now = e.timeStamp;
    if (travel >= MIN_TRAVEL_PX) turns.push(now);
    dir = d;
    travel = Math.abs(dx);
    turns = turns.filter((at) => now - at < WINDOW_MS);
    if (turns.length >= TURNS_NEEDED) onStroke(e.clientX, e.clientY);
  };
}
