// Workspace callers own timing and target selection; this only guards the DOM focus operation.
export function focusOutsideDialog(target: HTMLElement | null | undefined, options?: FocusOptions): void {
  if (!document.querySelector('dialog[open]')) target?.focus(options);
}
