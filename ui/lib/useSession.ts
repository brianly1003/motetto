"use client";
import { useEffect, useRef, useState } from "react";
import * as ipc from "@/lib/ipc";
import type { AppState } from "@/lib/bindings/AppState";
import type { Stage } from "@/lib/bindings/Stage";
import { markStage, startRun } from "@/lib/stageClock";

/** A pause in the live transcript: recorded time it ended at, and how long it lasted. */
export type Gap = { at_ms: number; ms: number };

export function useSession() {
  const [state, setState] = useState<AppState | null>(null);
  const [stage, setStage] = useState<Stage | null>(null);
  const [notice, setNotice] = useState("");
  const [pending, setPending] = useState(false);
  const meetingId = useRef<string | null>(null);
  // kept in the window only; a reload forgets the gap lines, the audio is unaffected.
  const [gaps, setGaps] = useState<Gap[]>([]);
  const pausedAt = useRef<number | null>(null);
  const finishing = useRef<string | null>(null); // the meeting whose notes are being written, for stageClock
  useEffect(() => {
    if (!ipc.desktopAvailable()) return;
    let active = true;
    let version = 0;
    const cleanups: Array<() => void> = [];
    const retain = (off: () => void) => active ? cleanups.push(off) : off();
    (async () => {
      retain(await ipc.onSessionEvent(event => {
        if (!active) return;
        if (event.type === "StateChanged") {
          version++;
          if (event.meeting_id !== meetingId.current) { setGaps([]); pausedAt.current = null; }
          meetingId.current = event.meeting_id;
          if (event.state.type === "Paused") pausedAt.current ??= Date.now();
          else if (pausedAt.current !== null) {
            const ms = Date.now() - pausedAt.current; pausedAt.current = null;
            if (event.state.type === "Recording") setGaps(list => [...list, { at_ms: event.elapsed_ms, ms }]);
          }
          if (event.state.type === "Finishing" && event.meeting_id && finishing.current !== event.meeting_id) startRun(event.meeting_id);
          finishing.current = event.state.type === "Finishing" ? event.meeting_id : null;
          setState(event); if (event.state.type !== "Finishing") setStage(null);
        }
        else if (event.type === "Stage" && event.meeting_id === meetingId.current) { markStage(event.meeting_id, event.stage.type); setStage(event.stage); }
      }));
      retain(await ipc.onNotice(text => { if (active) setNotice(text); }));
      const before = version;
      const current = await ipc.getState();
      if (active && version === before) { meetingId.current = current.meeting_id; setState(current); }
    })().catch(e => { if (active) setNotice(String(e)); });
    return () => { active = false; cleanups.forEach(off => off()); };
  }, []);
  async function act(command: () => Promise<void>) {
    setPending(true); setNotice("");
    try { await command(); } catch (e) { setNotice(String(e)); }
    finally { setPending(false); }
  }
  return { state, stage, notice, setNotice, pending, act, gaps };
}
