"use client";
import { Alert } from "@/components/Alert";
import { useEffect, useState } from "react";
import * as ipc from "@/lib/ipc";
import { strings as s } from "@/lib/strings";

export default function NotificationPermission() {
  const [allowed, setAllowed] = useState(true);
  const [message, setMessage] = useState(s.permissionBody);
  const [pending, setPending] = useState(false);
  useEffect(() => {
    if (!ipc.desktopAvailable()) return;
    let active = true;
    // Check again on focus: people allow Motetto in System Settings, then come back.
    const check = () => ipc.notificationPermission().then(value => { if (active) setAllowed(value); })
      .catch(error => { if (active) { setAllowed(false); setMessage(String(error)); } });
    check();
    window.addEventListener("focus", check);
    return () => { active = false; window.removeEventListener("focus", check); };
  }, []);
  async function request() {
    setPending(true);
    try { const value = await ipc.notificationPermission(true); setAllowed(value); if (!value) setMessage(s.notificationsDenied); }
    catch (error) { setMessage(String(error)); }
    finally { setPending(false); }
  }
  return !allowed && <Alert variant="warning" title="Recording notifications" actions={<button className="doc-action ghost" disabled={pending} onClick={request}>{s.allowNotifications}</button>}>{message}</Alert>;
}
