/** Helpers around native <dialog>. */

export function openDialog(el, { focus, closeOnBackdrop = true } = {}) {
  if (!(el instanceof HTMLDialogElement)) return;
  const previouslyFocused = document.activeElement;
  el.showModal();
  const target =
    focus ||
    el.querySelector("input, select, textarea, button:not([value='cancel'])");
  queueMicrotask(() => target?.focus?.());

  const onClose = () => {
    el.removeEventListener("close", onClose);
    if (previouslyFocused && previouslyFocused.focus) {
      try {
        previouslyFocused.focus();
      } catch {
        /* ignore */
      }
    }
  };
  el.addEventListener("close", onClose);

  if (!closeOnBackdrop) return;

  const onClick = (ev) => {
    if (ev.target === el) el.close();
  };
  el.addEventListener("click", onClick);
  el.addEventListener(
    "close",
    () => el.removeEventListener("click", onClick),
    { once: true },
  );
}

export function setBusy(form, busy) {
  if (!form) return;
  form.toggleAttribute("aria-busy", !!busy);
  for (const control of form.querySelectorAll("input, select, textarea, button")) {
    control.disabled = !!busy;
  }
}
