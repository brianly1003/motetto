"use client";
// The red frame around the recorded window or area (mockup .recframe / renderRecFrame). Rust
// (rec_frame.rs) places this transparent, click-through window around the target and hides it
// between parts; content protection keeps it out of every capture, the video included.
import { useSession } from "@/lib/useSession";
import "../../styles/recframe.css";

export default function RecFrame() {
  const { state } = useSession();
  const name = state?.screen?.while_sharing?.name ?? state?.screen?.pick.name ?? "";
  return <div className="recframe" data-label={`● REC${name ? ` · ${name}` : ""}`} aria-hidden="true"/>;
}
