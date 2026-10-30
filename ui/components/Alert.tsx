import type { ReactNode } from "react";
import { strings as s } from "@/lib/strings";

/** `onDismiss`: a close button in the top-right corner, not a button row (owner 2026-10-30: a lone
 *  Dismiss took a whole row). */
export function Alert({ title, children, actions, onDismiss, role = "status", className = "", variant = "info" }: {
  title: string; children: ReactNode; actions?: ReactNode; onDismiss?: () => void;
  role?: "status" | "alert" | "alertdialog"; className?: string;
  variant?: "info" | "success" | "warning" | "danger";
}) {
  return <div className={`banner alert ${className}`} data-variant={variant} role={role} aria-label={role === "alertdialog" ? title : undefined}>
    <svg className="alert-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true">{variant === "warning" ? <><path d="M10.3 3.9 2.2 18a2 2 0 0 0 1.7 3h16.2a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0Z"/><path d="M12 9v4m0 4h.01"/></> : <><circle cx="12" cy="12" r="9"/><path d={variant === "success" ? "m8 12 3 3 5-6" : variant === "danger" ? "m9 9 6 6m0-6-6 6" : "M12 11v5m0-9h.01"}/></>}</svg>
    <div className="alert-content"><div className="alert-title">{title}</div><div className="alert-description">{children}</div></div>
    {actions && <div className="alert-actions">{actions}</div>}
    {onDismiss && <button type="button" className="alert-close" aria-label={s.dismiss} title={s.dismiss} onClick={onDismiss}><svg viewBox="0 0 16 16" aria-hidden="true"><path d="m4.5 4.5 7 7m0-7-7 7" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/></svg></button>}
  </div>;
}
