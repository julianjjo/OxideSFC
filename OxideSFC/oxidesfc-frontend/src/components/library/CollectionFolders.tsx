import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '../common/Button';
import { Input } from '../common/Input';
import { Modal } from '../common/Modal';

interface CollectionFoldersProps {
  theme?: 'dark' | 'light';
  onSelectCollection?: (collectionId: string | null) => void;
  selectedCollectionId?: string | null;
  onGameDrop?: (gameId: string, collectionId: string) => void;
}

interface GameFolder {
  id: string;
  name: string;
  parent_id: string | null;
  created_at: string;
  game_count?: number;
}

interface Game {
  id: string;
  title: string;
  file_name: string;
}

export function CollectionFolders({
  theme = 'dark',
  onSelectCollection,
  selectedCollectionId,
  onGameDrop,
}: CollectionFoldersProps) {
  const [folders, setFolders] = useState<GameFolder[]>([]);
  const [games, setGames] = useState<Game[]>([]);
  const [selectedFolder, setSelectedFolder] = useState<string | null>(selectedCollectionId || null);
  const [isLoading, setIsLoading] = useState(true);
  const [showCreateModal, setShowCreateModal] = useState(false);
  const [showRenameModal, setShowRenameModal] = useState(false);
  const [showDeleteModal, setShowDeleteModal] = useState(false);
  const [editingFolder, setEditingFolder] = useState<GameFolder | null>(null);
  const [newFolderName, setNewFolderName] = useState('');
  const [draggedGameId, setDraggedGameId] = useState<string | null>(null);
  const [isDragOver, setIsDragOver] = useState<string | null>(null);

  useEffect(() => {
    loadFolders();
    loadAllGames();
  }, []);

  const loadFolders = async () => {
    try {
      setIsLoading(true);
      const result = await invoke<GameFolder[]>('get_folders');
      setFolders(result);
    } catch (error) {
      console.error('Failed to load folders:', error);
    } finally {
      setIsLoading(false);
    }
  };

  const loadAllGames = async () => {
    try {
      const result = await invoke<Game[]>('get_games');
      setGames(result);
    } catch (error) {
      console.error('Failed to load games:', error);
    }
  };

  const loadGamesInFolder = async (folderId: string) => {
    try {
      const result = await invoke<Game[]>('get_games_in_folder', { folderId });
      setGames(result);
    } catch (error) {
      console.error('Failed to load games in folder:', error);
    }
  };

  const handleCreateFolder = async () => {
    if (!newFolderName.trim()) return;
    
    try {
      await invoke('create_folder', { name: newFolderName.trim() });
      await loadFolders();
      setShowCreateModal(false);
      setNewFolderName('');
    } catch (error) {
      console.error('Failed to create folder:', error);
    }
  };

  const handleRenameFolder = async () => {
    if (!editingFolder || !newFolderName.trim()) return;
    
    try {
      await invoke('rename_folder', { 
        folderId: editingFolder.id, 
        name: newFolderName.trim() 
      });
      await loadFolders();
      setShowRenameModal(false);
      setEditingFolder(null);
      setNewFolderName('');
    } catch (error) {
      console.error('Failed to rename folder:', error);
    }
  };

  const handleDeleteFolder = async () => {
    if (!editingFolder) return;
    
    try {
      await invoke('delete_folder', { folderId: editingFolder.id });
      await loadFolders();
      if (selectedFolder === editingFolder.id) {
        setSelectedFolder(null);
        onSelectCollection?.(null);
        loadAllGames();
      }
      setShowDeleteModal(false);
      setEditingFolder(null);
    } catch (error) {
      console.error('Failed to delete folder:', error);
    }
  };

  const handleSelectFolder = (folderId: string | null) => {
    setSelectedFolder(folderId);
    onSelectCollection?.(folderId);
    
    if (folderId) {
      loadGamesInFolder(folderId);
    } else {
      loadAllGames();
    }
  };

  const handleStartRename = (folder: GameFolder) => {
    setEditingFolder(folder);
    setNewFolderName(folder.name);
    setShowRenameModal(true);
  };

  const handleStartDelete = (folder: GameFolder) => {
    setEditingFolder(folder);
    setShowDeleteModal(true);
  };

  // Drag and Drop handlers
  const handleDragStart = (e: React.DragEvent, gameId: string) => {
    e.dataTransfer.setData('gameId', gameId);
    setDraggedGameId(gameId);
  };

  const handleDragEnd = () => {
    setDraggedGameId(null);
    setIsDragOver(null);
  };

  const handleDragOver = (e: React.DragEvent, folderId: string) => {
    e.preventDefault();
    setIsDragOver(folderId);
  };

  const handleDragLeave = () => {
    setIsDragOver(null);
  };

  const handleDrop = async (e: React.DragEvent, folderId: string) => {
    e.preventDefault();
    const gameId = e.dataTransfer.getData('gameId');
    
    if (gameId) {
      try {
        await invoke('add_game_to_folder', { gameId, folderId });
        onGameDrop?.(gameId, folderId);
        await loadFolders();
      } catch (error) {
        console.error('Failed to add game to folder:', error);
      }
    }
    
    setIsDragOver(null);
    setDraggedGameId(null);
  };

  const handleRemoveGameFromFolder = async (gameId: string) => {
    if (!selectedFolder) return;
    
    try {
      await invoke('remove_game_from_folder', { gameId, folderId: selectedFolder });
      setGames(games.filter(g => g.id !== gameId));
    } catch (error) {
      console.error('Failed to remove game from folder:', error);
    }
  };

  const containerClass = theme === 'light' 
    ? 'bg-white border-gray-200' 
    : 'bg-slate-800 border-slate-700';

  const textClass = theme === 'light' 
    ? 'text-gray-700' 
    : 'text-slate-200';

  const mutedClass = theme === 'light' 
    ? 'text-gray-500' 
    : 'text-slate-400';

  const hoverClass = theme === 'light' 
    ? 'hover:bg-gray-100' 
    : 'hover:bg-slate-700';

  return (
    <div className={`h-full flex flex-col rounded-lg border ${containerClass}`}>
      {/* Header */}
      <div className={`p-4 border-b ${theme === 'light' ? 'border-gray-200' : 'border-slate-700'}`}>
        <div className="flex items-center justify-between">
          <h2 className={`font-semibold ${textClass}`}>Collections</h2>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setShowCreateModal(true)}
          >
            + New
          </Button>
        </div>
      </div>

      {/* All Games */}
      <div className="p-2">
        <button
          onClick={() => handleSelectFolder(null)}
          className={`w-full text-left px-3 py-2 rounded-lg flex items-center justify-between ${
            selectedFolder === null ? 'bg-primary-600 text-white' : `${hoverClass}`
          }`}
        >
          <span className="flex items-center gap-2">
            <span>📚</span>
            <span className={textClass}>All Games</span>
          </span>
          <span className={`text-xs ${selectedFolder === null ? 'text-white/70' : mutedClass}`}>
            {games.length}
          </span>
        </button>
      </div>

      {/* Folders List */}
      <div className="flex-1 overflow-auto p-2 pt-0">
        {isLoading ? (
          <div className="p-4 text-center">
            <span className={mutedClass}>Loading...</span>
          </div>
        ) : folders.length === 0 ? (
          <div className="p-4 text-center">
            <p className={`text-sm ${mutedClass}`}>No collections yet</p>
            <Button
              variant="ghost"
              size="sm"
              className="mt-2"
              onClick={() => setShowCreateModal(true)}
            >
              Create Collection
            </Button>
          </div>
        ) : (
          <div className="space-y-1">
            {folders.map((folder) => (
              <div
                key={folder.id}
                className={`group relative rounded-lg ${
                  selectedFolder === folder.id 
                    ? 'bg-primary-600 text-white' 
                    : `${hoverClass}`
                }`}
                onDragOver={(e) => handleDragOver(e, folder.id)}
                onDragLeave={handleDragLeave}
                onDrop={(e) => handleDrop(e, folder.id)}
              >
                <button
                  onClick={() => handleSelectFolder(folder.id)}
                  className="w-full text-left px-3 py-2 flex items-center justify-between"
                >
                  <span className="flex items-center gap-2">
                    <span>📁</span>
                    <span className={selectedFolder === folder.id ? 'text-white' : textClass}>
                      {folder.name}
                    </span>
                  </span>
                  <span className={`text-xs ${
                    selectedFolder === folder.id ? 'text-white/70' : mutedClass
                  }`}>
                    {folder.game_count || 0}
                  </span>
                </button>
                
                {/* Hover Actions */}
                <div className={`absolute right-1 top-1/2 -translate-y-1/2 hidden group-hover:flex gap-1 ${
                  selectedFolder === folder.id ? 'text-white' : ''
                }`}>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleStartRename(folder);
                    }}
                    className={`p-1 rounded ${theme === 'light' ? 'hover:bg-gray-200' : 'hover:bg-slate-600'}`}
                    title="Rename"
                  >
                    ✏️
                  </button>
                  <button
                    onClick={(e) => {
                      e.stopPropagation();
                      handleStartDelete(folder);
                    }}
                    className={`p-1 rounded ${theme === 'light' ? 'hover:bg-gray-200' : 'hover:bg-slate-600'}`}
                    title="Delete"
                  >
                    🗑️
                  </button>
                </div>
                
                {/* Drop indicator */}
                {isDragOver === folder.id && (
                  <div className="absolute inset-0 border-2 border-dashed border-primary-500 rounded-lg bg-primary-500/20" />
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Games in selected folder */}
      {selectedFolder && games.length > 0 && (
        <div className={`p-4 border-t ${theme === 'light' ? 'border-gray-200' : 'border-slate-700'}`}>
          <h3 className={`text-sm font-medium mb-2 ${mutedClass}`}>Games in collection</h3>
          <div className="space-y-1 max-h-48 overflow-auto">
            {games.map((game) => (
              <div
                key={game.id}
                draggable
                onDragStart={(e) => handleDragStart(e, game.id)}
                onDragEnd={handleDragEnd}
                className={`flex items-center justify-between px-2 py-1 rounded text-sm ${
                  theme === 'light' ? 'bg-gray-100' : 'bg-slate-700'
                } ${draggedGameId === game.id ? 'opacity-50' : ''}`}
              >
                <span className={textClass}>{game.title}</span>
                <button
                  onClick={() => handleRemoveGameFromFolder(game.id)}
                  className={`p-1 rounded ${theme === 'light' ? 'hover:bg-gray-200' : 'hover:bg-slate-600'}`}
                  title="Remove from collection"
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* Create Folder Modal */}
      <Modal
        isOpen={showCreateModal}
        onClose={() => {
          setShowCreateModal(false);
          setNewFolderName('');
        }}
        title="Create Collection"
        footer={
          <>
            <Button variant="ghost" onClick={() => setShowCreateModal(false)}>
              Cancel
            </Button>
            <Button variant="primary" onClick={handleCreateFolder}>
              Create
            </Button>
          </>
        }
      >
        <Input
          label="Collection Name"
          value={newFolderName}
          onChange={(e) => setNewFolderName(e.target.value)}
          placeholder="Enter collection name"
          onKeyDown={(e) => e.key === 'Enter' && handleCreateFolder()}
        />
      </Modal>

      {/* Rename Folder Modal */}
      <Modal
        isOpen={showRenameModal}
        onClose={() => {
          setShowRenameModal(false);
          setEditingFolder(null);
          setNewFolderName('');
        }}
        title="Rename Collection"
        footer={
          <>
            <Button variant="ghost" onClick={() => setShowRenameModal(false)}>
              Cancel
            </Button>
            <Button variant="primary" onClick={handleRenameFolder}>
              Rename
            </Button>
          </>
        }
      >
        <Input
          label="New Name"
          value={newFolderName}
          onChange={(e) => setNewFolderName(e.target.value)}
          placeholder="Enter new name"
          onKeyDown={(e) => e.key === 'Enter' && handleRenameFolder()}
        />
      </Modal>

      {/* Delete Folder Modal */}
      <Modal
        isOpen={showDeleteModal}
        onClose={() => {
          setShowDeleteModal(false);
          setEditingFolder(null);
        }}
        title="Delete Collection"
        footer={
          <>
            <Button variant="ghost" onClick={() => setShowDeleteModal(false)}>
              Cancel
            </Button>
            <Button variant="danger" onClick={handleDeleteFolder}>
              Delete
            </Button>
          </>
        }
      >
        <p className={textClass}>
          Are you sure you want to delete "{editingFolder?.name}"? 
          Games in this collection will not be deleted.
        </p>
      </Modal>
    </div>
  );
}
