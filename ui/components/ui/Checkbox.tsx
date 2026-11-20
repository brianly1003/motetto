import { useEffect, useRef, type InputHTMLAttributes, type ReactNode } from "react";

// Atoms: the checkbox, after shadcn/ui's (DESIGN 4.0). A real <input type="checkbox">, drawn by
// atoms.css (`.checkbox`) so it looks the same in every engine: 16 px, 4 px corners, the input
// border; checked or mixed it fills with the primary ink and shows a check or a dash. Keyboard focus
// shows as hover does, never as a ring. Use CheckboxField for a label and an optional description.
type CheckboxProps = Omit<InputHTMLAttributes<HTMLInputElement>, "type" | "onChange"> & {
  onCheckedChange?: (checked: boolean) => void;
  /** Some but not all: shows a dash (the input's `indeterminate`). */
  indeterminate?: boolean;
  /** Shows the danger border; pair with CheckboxField `invalid` for the label. */
  invalid?: boolean;
};

export function Checkbox({ onCheckedChange, indeterminate = false, invalid, className, ...props }: CheckboxProps) {
  const box = useRef<HTMLInputElement>(null);
  useEffect(() => { if (box.current) box.current.indeterminate = indeterminate; }, [indeterminate]);
  return <input ref={box} type="checkbox" className={`checkbox${className ? ` ${className}` : ""}`} aria-invalid={invalid || undefined}
    onChange={event => onCheckedChange?.(event.currentTarget.checked)} {...props}/>;
}

/** A checkbox with its label (and a description under it); the whole row toggles it. */
export function CheckboxField({ label, description, invalid, disabled, className, ...props }: CheckboxProps & { label: ReactNode; description?: ReactNode }) {
  return <label className={`field-check${className ? ` ${className}` : ""}`} data-invalid={invalid || undefined} data-disabled={disabled || undefined}>
    <Checkbox invalid={invalid} disabled={disabled} {...props}/>
    <span className="field-check-text"><span className="field-check-label">{label}</span>{description && <span className="field-check-desc">{description}</span>}</span>
  </label>;
}
