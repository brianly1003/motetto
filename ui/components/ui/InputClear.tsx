import type { InputHTMLAttributes, Ref } from "react";

// Atoms: a text field with a clear button inside its right edge (atoms.css `.input-clear`). The
// button shows only while the field has text (it hides with the placeholder), and clearing hands
// focus back to the field. A placeholder is required: it is what hides the button when empty.
type InputClearProps = Omit<InputHTMLAttributes<HTMLInputElement>, "value" | "onChange" | "placeholder"> & {
  value: string;
  onValueChange: (value: string) => void;
  placeholder: string;
  clearLabel: string;
  ref?: Ref<HTMLInputElement>;
};

const X = <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" aria-hidden="true"><path d="M18 6 6 18M6 6l12 12"/></svg>;

export function InputClear({ value, onValueChange, clearLabel, className, type = "text", ref, ...props }: InputClearProps) {
  return <span className={`input-clear${className ? ` ${className}` : ""}`}>
    <input ref={ref} type={type} className="field-input" value={value} onChange={event => onValueChange(event.target.value)} {...props}/>
    <button type="button" tabIndex={-1} aria-label={clearLabel} title={clearLabel} onMouseDown={event => event.preventDefault()}
      onClick={event => { onValueChange(""); (event.currentTarget.previousElementSibling as HTMLInputElement | null)?.focus(); }}>{X}</button>
  </span>;
}
