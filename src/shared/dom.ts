// Tiny helper to build DOM elements. Text is always inserted as text (never as
// HTML), so content coming from Claude can't inject markup or scripts.

type Child = Node | string | null | undefined | false;

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Record<string, string | number | boolean | undefined> = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value === false || value === undefined) continue;
    if (key === "class") node.className = String(value);
    else if (key === "text") node.textContent = String(value);
    else node.setAttribute(key, value === true ? "" : String(value));
  }
  for (const child of children) if (child) node.append(child);
  return node;
}

/** Drops null / false entries so optional parts can be written inline. */
export const compact = (...items: Child[]): (Node | string)[] => items.filter((x): x is Node | string => !!x);

export function button(label: string, variant: string, onClick: () => void, title?: string) {
  const b = el("button", { class: `btn ${variant}`.trim(), type: "button", text: label, title });
  b.addEventListener("click", (e) => {
    e.stopPropagation();
    onClick();
  });
  return b;
}
