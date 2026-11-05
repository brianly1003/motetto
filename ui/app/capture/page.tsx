"use client";
// The screen setup overlay, one transparent window per display (Rust: capture_overlay.rs). This
// placeholder only closes on Esc; the setup bar replaces it.
import { useEffect } from "react";
import * as ipc from "@/lib/ipc";

export default function Capture() {
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") void ipc.closeCapture(); };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  }, []);
  return <><style>{"html,body{background:transparent!important}"}</style><main aria-label="Screen setup" style={{ position: "fixed", inset: 0 }} /></>;
}
