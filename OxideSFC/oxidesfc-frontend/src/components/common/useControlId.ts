import { createContext, useContext, useId } from 'react';

/**
 * The id a surrounding row's `<label>` points at, offered to the control inside
 * it.
 *
 * Lives in `common/` rather than next to `SettingRow` so the dependency runs in
 * the right direction: the primitives here must not import from a feature folder.
 *
 * It exists because a settings row lays its label and its control out as flex
 * siblings, so the label cannot wrap the control and has to reference it by id.
 * `SettingRow` used to take an `htmlFor` prop for that, and not one of the ~60
 * call sites across the five panels ever passed it — so every settings label was
 * an orphan and clicking the words "Vertical sync" or "Include subfolders" did
 * nothing, where a wired label would toggle the switch. Publishing the id here
 * wires the two together without asking every call site to thread one by hand.
 */
export const SettingRowIdContext = createContext<string | undefined>(undefined);

/**
 * The id a form control should carry.
 *
 * Resolution order: an explicit `id` prop wins, then the id published by an
 * enclosing row, then a generated fallback. `useId` is called unconditionally,
 * since hooks cannot be skipped.
 */
export function useControlId(explicitId: string | undefined, prefix: string): string {
  const generated = useId();
  const rowId = useContext(SettingRowIdContext);
  return explicitId ?? rowId ?? `${prefix}-${generated}`;
}
