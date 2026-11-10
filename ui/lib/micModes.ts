import type { MicMode } from "./bindings/MicMode";

/** Settings › Audio › Microphone mode, in order: name and one line on what it does. */
export const MIC_MODES: [MicMode, string, string][] = [
  ["standard", "Standard", "Removes the call's echo when your mic hears your speakers."],
  ["reduce_noise", "Reduce noise", "Also lowers steady noise, like a fan or hum, in what you play back. The transcript keeps your whole voice."],
  ["original", "Original", "Your mic exactly as recorded, with music and room sound."],
];
export const micModeName = (mode: MicMode) => MIC_MODES.find(([id]) => id === mode)?.[1] ?? "Standard";
