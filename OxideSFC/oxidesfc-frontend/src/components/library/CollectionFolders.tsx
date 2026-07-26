import { useCallback, useEffect, useState, type DragEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '../common/Button';
import { Input } from '../common/Input';
import { Modal, ConfirmModal } from '../common/Modal';
import { IconFolder, IconPlus, IconPencil, IconTrash } from '../common/icons';

interface GameFolder {
  id: string;
  name: string;
  parent_id: string | null;
  created_at: string;
  game_count?: number;
}

interface CollectionFoldersProps {
  selectedId: string | null;
  onSelect: (collectionId: string | null) => void;
  /** Called after a game is filed, so the library can refresh its counts. */
  onGameFiled?: (gameId: string, collectionId: string) => void;
}

/**
 * User-made collections.
 *
 * Games are filed by dragging a card (or a list row) onto a collection: the drag
 * source is the game you are looking at in the shelf. This component used to
 * carry its own list of games inside the selected collection purely to have
 * something draggable, which meant the shelf and the sidebar showed overlapping
 * copies of the same set.
 */
export function CollectionFolders({
  selectedId,
  onSelect,
  onGameFiled,
}: CollectionFoldersProps) {
  const [folders, setFolders] = useState<GameFolder[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [dropTarget, setDropTarget] = useState<string | null>(null);

  const [creating, setCreating] = useState(false);
  const [renaming, setRenaming] = useState<GameFolder | null>(null);
  const [deleting, setDeleting] = useState<GameFolder | null>(null);
  const [draftName, setDraftName] = useState('');

  const loadFolders = useCallback(async () => {
    try {
      setIsLoading(true);
      setFolders(await invoke<GameFolder[]>('get_folders'));
    } catch (error) {
      console.error('Failed to load collections:', error);
    } finally {
      setIsLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadFolders();
  }, [loadFolders]);

  const handleCreate = async () => {
    const name = draftName.trim();
    if (!name) return;
    try {
      await invoke('create_folder', { name });
      await loadFolders();
    } catch (error) {
      console.error('Failed to create collection:', error);
    }
    setCreating(false);
    setDraftName('');
  };

  const handleRename = async () => {
    const name = draftName.trim();
    if (!renaming || !name) return;
    try {
      await invoke('rename_folder', { folderId: renaming.id, name });
      await loadFolders();
    } catch (error) {
      console.error('Failed to rename collection:', error);
    }
    setRenaming(null);
    setDraftName('');
  };

  const handleDelete = async () => {
    if (!deleting) return;
    try {
      await invoke('delete_folder', { folderId: deleting.id });
      // Deleting the active collection has to clear the selection, or the shelf
      // keeps filtering against an id the backend no longer knows and shows an
      // empty library with no visible reason.
      if (selectedId === deleting.id) onSelect(null);
      await loadFolders();
    } catch (error) {
      console.error('Failed to delete collection:', error);
    }
    setDeleting(null);
  };

  const handleDrop = async (e: DragEvent, folderId: string) => {
    e.preventDefault();
    setDropTarget(null);
    const gameId = e.dataTransfer.getData('gameId');
    if (!gameId) return;
    try {
      await invoke('add_game_to_folder', { gameId, folderId });
      onGameFiled?.(gameId, folderId);
      await loadFolders();
    } catch (error) {
      console.error('Failed to file game into collection:', error);
    }
  };

  return (
    <div>
      <div className="mb-2 flex items-center justify-between px-1">
        <p className="eyebrow">Collections</p>
        <button
          type="button"
          onClick={() => {
            setDraftName('');
            setCreating(true);
          }}
          className="text-mute transition-colors hover:text-ink"
          title="New collection"
          aria-label="New collection"
        >
          <IconPlus size={14} />
        </button>
      </div>

      {isLoading ? (
        <p className="register px-2">loading…</p>
      ) : folders.length === 0 ? (
        <p className="px-2 text-[0.8125rem] leading-relaxed text-mute">
          No collections yet. Make one, then drag games onto it.
        </p>
      ) : (
        <ul className="space-y-0.5">
          {folders.map((folder) => {
            const active = selectedId === folder.id;
            const isDropTarget = dropTarget === folder.id;
            return (
              <li key={folder.id}>
                <div
                  onDragOver={(e) => {
                    e.preventDefault();
                    setDropTarget(folder.id);
                  }}
                  onDragLeave={() => setDropTarget(null)}
                  onDrop={(e) => void handleDrop(e, folder.id)}
                  className={`group flex items-center rounded-md transition-colors ${
                    isDropTarget
                      ? 'bg-accent-soft ring-1 ring-accent'
                      : active
                        ? 'bg-accent-soft'
                        : 'hover:bg-raised'
                  }`}
                >
                  <button
                    type="button"
                    onClick={() => onSelect(active ? null : folder.id)}
                    aria-current={active ? 'true' : undefined}
                    className={`flex min-w-0 flex-1 items-center gap-2.5 px-2 py-1.5 text-left text-[0.8125rem] font-semibold ${
                      active ? 'text-accent-text' : 'text-dim group-hover:text-ink'
                    }`}
                  >
                    <span className="flex-none">
                      <IconFolder size={16} />
                    </span>
                    <span className="min-w-0 flex-1 truncate">{folder.name}</span>
                    <span className="register flex-none">{folder.game_count ?? 0}</span>
                  </button>

                  <span className="flex flex-none items-center pr-1 opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
                    <button
                      type="button"
                      onClick={() => {
                        setDraftName(folder.name);
                        setRenaming(folder);
                      }}
                      className="rounded p-1 text-mute hover:text-ink"
                      title={`Rename ${folder.name}`}
                      aria-label={`Rename ${folder.name}`}
                    >
                      <IconPencil size={13} />
                    </button>
                    <button
                      type="button"
                      onClick={() => setDeleting(folder)}
                      className="rounded p-1 text-mute hover:text-danger-text"
                      title={`Delete ${folder.name}`}
                      aria-label={`Delete ${folder.name}`}
                    >
                      <IconTrash size={13} />
                    </button>
                  </span>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      <Modal
        isOpen={creating}
        onClose={() => setCreating(false)}
        title="New collection"
        size="sm"
        footer={
          <>
            <Button variant="ghost" onClick={() => setCreating(false)}>
              Cancel
            </Button>
            <Button onClick={handleCreate} disabled={!draftName.trim()}>
              Create
            </Button>
          </>
        }
      >
        <Input
          label="Name"
          value={draftName}
          onChange={(e) => setDraftName(e.target.value)}
          placeholder="Platformers, RPGs to finish…"
          onKeyDown={(e) => e.key === 'Enter' && handleCreate()}
        />
      </Modal>

      <Modal
        isOpen={renaming !== null}
        onClose={() => setRenaming(null)}
        title="Rename collection"
        size="sm"
        footer={
          <>
            <Button variant="ghost" onClick={() => setRenaming(null)}>
              Cancel
            </Button>
            <Button onClick={handleRename} disabled={!draftName.trim()}>
              Rename
            </Button>
          </>
        }
      >
        <Input
          label="Name"
          value={draftName}
          onChange={(e) => setDraftName(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleRename()}
        />
      </Modal>

      <ConfirmModal
        isOpen={deleting !== null}
        onClose={() => setDeleting(null)}
        onConfirm={handleDelete}
        title={`Delete “${deleting?.name ?? ''}”?`}
        message="The collection goes; the games in it stay in your library."
        confirmText="Delete collection"
        variant="danger"
      />
    </div>
  );
}
