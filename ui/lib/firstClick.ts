import { useEffect, type RefObject } from "react";

// Some macOS input methods (OpenKey in VI mode) reorder a click while a text field has focus: the
// page gets the release before the press, so that press pairs with the next click's release and
// every control needs two clicks. Inside `root`, a release on a control (not after a press that
// began somewhere else) activates it at once, and the browser's own click that may follow for the
// same press is dropped, so a control never runs twice. Keyboard clicks are untouched.
const CONTROL = "button, [role=button], [role=radio], label, input[type=checkbox]";
const FOLLOW_MS = 600, LATE_PRESS_MS = 100;

export function useFirstClick(root: RefObject<HTMLElement | null>) {
  useEffect(() => {
    const box = root.current;
    if (!box) return;
    let pressed: Element | null = null, done: { target: Element; until: number } | null = null, released: { target: Element; at: number } | null = null;
    const control = (target: EventTarget | null) => target instanceof Element ? target.closest(CONTROL) : null;
    // A press anywhere (a field's text too) is remembered, so a drag that ends on a control does not
    // activate it. A press just after a release on the same control is that click's late half.
    const press = (event: Event) => {
      const target = control(event.target) ?? (event.target instanceof Element ? event.target : null);
      pressed = released && released.target === target && performance.now() - released.at < LATE_PRESS_MS ? null : target;
    };
    const release = (event: PointerEvent) => {
      if (event.button !== 0) return;
      const target = control(event.target), was = pressed;
      pressed = null;
      released = target ? { target, at: performance.now() } : null;
      if (!target || !box.contains(target) || (was && was !== target) || target.matches(":disabled")) return;
      done = { target, until: performance.now() + FOLLOW_MS };
      (target as HTMLElement).click();
    };
    const click = (event: MouseEvent) => {
      if (!event.isTrusted || !done || event.detail === 0) return;
      if (performance.now() < done.until && done.target.contains(event.target as Node)) { event.preventDefault(); event.stopPropagation(); }
      done = null;
    };
    box.addEventListener("pointerdown", press, true); box.addEventListener("mousedown", press, true);
    box.addEventListener("pointerup", release, true); box.addEventListener("click", click, true);
    return () => {
      box.removeEventListener("pointerdown", press, true); box.removeEventListener("mousedown", press, true);
      box.removeEventListener("pointerup", release, true); box.removeEventListener("click", click, true);
    };
  }, [root]);
}
