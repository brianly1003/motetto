// When a meeting's notes run began and when each of its stages did (Date.now() ms), recorded as the
// events arrive, so a steps page opened partway through still shows true times (owner 2026-10-25).
// Kept in the window only: after a reload the steps show no times until the next run.
const runs = new Map<string, Record<string, number>>();

export function startRun(id: string) { runs.set(id, { start: Date.now() }); }

/** The first time a stage of the run was reported; later reports of the same stage don't move it. */
export function markStage(id: string, type: string) { const run = runs.get(id); if (run) run[type] ??= Date.now(); }

export function stageTimes(id: string): Record<string, number | undefined> { return runs.get(id) ?? {}; }
