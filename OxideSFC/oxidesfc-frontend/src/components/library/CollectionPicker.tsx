import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { IconFolder, IconCheck } from '../common/icons';

interface GameFolder {
  id: string;
  name: string;
}

interface CollectionPickerProps {
  gameId: string;
  /** Called after membership changes, so the library can refresh its counts. */
  onChange?: () => void;
}

/**
 * Keyboard-accessible collection membership for one game.
 *
 * The shelf files games by dragging a card onto a collection, which is a good
 * pointer gesture and *only* a pointer gesture: it is unusable by keyboard, by
 * screen reader, and by anyone who finds drag-and-drop awkward. Since the
 * redesign also dropped the old sidebar list that used to be the drag source,
 * dragging had become the only route in — so this is the non-mouse path, and it
 * doubles as the discoverable one for anyone who never guesses that cards drag.
 */
export function CollectionPicker({ gameId, onChange }: CollectionPickerProps) {
  const [folders, setFolders] = useState<GameFolder[]>([]);
  const [memberOf, setMemberOf] = useState<Set<string>>(new Set());
  const [isLoading, setIsLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setIsLoading(true);
      const [allFolders, mine] = await Promise.all([
        invoke<GameFolder[]>('get_folders'),
        invoke<string[]>('get_folders_for_game', { gameId }),
      ]);
      setFolders(allFolders);
      setMemberOf(new Set(mine));
    } catch (error) {
      console.error('Failed to load collections:', error);
      setFolders([]);
      setMemberOf(new Set());
    } finally {
      setIsLoading(false);
    }
  }, [gameId]);

  useEffect(() => {
    void load();
  }, [load]);

  const toggle = async (folder: GameFolder) => {
    const isMember = memberOf.has(folder.id);
    setBusyId(folder.id);

    // Optimistic: the checkbox has to respond to the keypress immediately, and
    // the command is a small local file write that rarely fails.
    setMemberOf((previous) => {
      const next = new Set(previous);
      if (isMember) next.delete(folder.id);
      else next.add(folder.id);
      return next;
    });

    try {
      await invoke(isMember ? 'remove_game_from_folder' : 'add_game_to_folder', {
        gameId,
        folderId: folder.id,
      });
      onChange?.();
    } catch (error) {
      console.error('Failed to update collection membership:', error);
      // Put the checkbox back rather than leaving it showing a state that was
      // never persisted.
      setMemberOf((previous) => {
        const next = new Set(previous);
        if (isMember) next.add(folder.id);
        else next.delete(folder.id);
        return next;
      });
    } finally {
      setBusyId(null);
    }
  };

  if (isLoading) {
    return <p className="hint">loading collections…</p>;
  }

  if (folders.length === 0) {
    return (
      <p className="field-row-help">
        No collections yet. Create one from the library sidebar, then add games to
        it here or by dragging them onto it.
      </p>
    );
  }

  return (
    <ul className="space-y-1">
      {folders.map((folder) => {
        const isMember = memberOf.has(folder.id);
        return (
          <li key={folder.id}>
            <button
              type="button"
              onClick={() => void toggle(folder)}
              disabled={busyId === folder.id}
              aria-pressed={isMember}
              className={`flex w-full items-center gap-2.5 rounded-md border px-3 py-2 text-left text-[0.8125rem] font-semibold transition-colors disabled:opacity-50 ${
                isMember
                  ? 'border-accent-line bg-accent-soft text-accent-text'
                  : 'border-line bg-raised text-dim hover:border-line-strong hover:text-ink'
              }`}
            >
              <span className="flex-none">
                <IconFolder size={15} />
              </span>
              <span className="min-w-0 flex-1 truncate">{folder.name}</span>
              {isMember && (
                <span className="flex-none" aria-hidden>
                  <IconCheck size={14} />
                </span>
              )}
            </button>
          </li>
        );
      })}
    </ul>
  );
}
