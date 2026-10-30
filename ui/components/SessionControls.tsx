"use client";
import { Alert } from "@/components/Alert";
import { useSession } from "@/lib/useSession";
import * as ipc from "@/lib/ipc";
import { elapsedLabel, problemMessage } from "@/lib/session";
import { strings as s } from "@/lib/strings";

// hideActive: on any meeting page; the live meeting's page shows its bar and the sidebar chip
// (Recording, Paused, Writing notes) already says what the live one is doing (owner, 2026-10-20).
export default function SessionControls({ hideActive = false }: { hideActive?: boolean }) {
  const { state, stage, notice, setNotice, pending, act } = useSession();
  const recording = state?.state.type === "Recording";
  const paused = state?.state.type === "Paused";
  const stopped = state?.state.type === "Stopped";
  const finishing = state?.state.type === "Finishing";
  return <>
    {(recording || paused || stopped || finishing) && !hideActive && <div className={`banner session-banner${recording ? " is-recording" : ""}`} role="status">
      <span>{recording ? s.recording : paused ? s.pillPaused : stopped ? s.stopped : stage?.type === "Transcribing" ? s.transcribing : s.processing} {state && !finishing && <time>{elapsedLabel(state.elapsed_ms)}</time>}</span>
      {recording && <button className="quiet" disabled={pending} onClick={() => act(ipc.pauseRecording)}>{s.pause}</button>}
      {paused && <button className="quiet" disabled={pending} onClick={() => act(ipc.resumeRecording)}>{s.resume}</button>}
      {(recording || paused) && <button className="quiet" disabled={pending} onClick={() => act(ipc.stopRecording)}>{s.stop}</button>}
      {stopped && <button className="quiet" disabled={pending} onClick={() => act(ipc.resumeRecording)}>{s.resume}</button>}
    </div>}
    {state?.quit_pending && <Alert variant={finishing ? "info" : "warning"} title={finishing ? "Saving before quitting" : "Keep Motetto open?"} role="alert" actions={<>
      {!finishing && <button className="doc-action ghost" disabled={pending} onClick={() => act(() => ipc.quitRecording(true))}>{s.stopAndSave}</button>}
      <button className="doc-action ghost" disabled={pending} onClick={() => act(() => ipc.quitRecording(false))}>{recording ? s.keepRecording : s.stayOpen}</button>
    </>}>{finishing ? s.quitPending : s.quitQuestion}</Alert>}
    {state?.problem && <Alert variant={["DiskFull", "ToolFailed", "NotesFailed"].includes(state.problem.problem.type) ? "danger" : "warning"} title={s.error} role="alert" actions={<>{state.problem.problem.type==="PermissionMissing"&&<button className="doc-action ghost" disabled={pending} onClick={()=>{const p=state.problem!.problem;if(p.type==="PermissionMissing")void act(()=>ipc.openPermissionSettings(p.kind));}}>Open settings</button>}{state.problem.problem.type==='ModelMissing'&&<button className="doc-action ghost" disabled={pending} onClick={()=>act(()=>ipc.openSettings('models'))}>Open Models</button>}{['DiskLow','DiskFull'].includes(state.problem.problem.type)&&<button className="doc-action ghost" disabled={pending} onClick={()=>act(()=>ipc.openSettings('privacy'))}>Free up space</button>}</>} onDismiss={pending ? undefined : () => act(() => ipc.dismissProblem(state.problem!.id))}>{problemMessage(state.problem.problem)}</Alert>}
    {notice && <Alert title="Motetto" onDismiss={() => setNotice("")}>{notice}</Alert>}
  </>;
}
