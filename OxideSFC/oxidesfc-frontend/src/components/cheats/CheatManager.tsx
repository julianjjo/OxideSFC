/**
 * Cheat Manager Component
 * 
 * Manages cheat codes for games with support for:
 * - List of cheat codes per game
 * - Add/edit/delete cheats
 * - Enable/disable cheats toggle
 * - Import/export cheat databases
 * - Common SNES cheat code formats supported
 */

import { useState, useEffect, useRef } from 'react';
import { Button } from '../common/Button';
import { Toggle } from '../common/Toggle';
import { Input } from '../common/Input';
import { Modal } from '../common/Modal';
import { Select } from '../common/Select';
import { TextArea } from '../common/Input';
import type { Game } from '../../domain/types';

import type { 
  CheatCode, 
  CheatCodeFormat, 
  CheatCategory 
} from './types';
import {
  validateCheatCode,
  CHEAT_CATEGORY_LABELS,
} from './types';

// ============================================================================
// Database Constants
// ============================================================================

const DB_NAME = 'oxidesfc-cheats';
const STORE_NAME = 'cheats';

// ============================================================================
// Cheat Manager Component
// ============================================================================

export interface CheatManagerProps {
  game: Game | null;
  isOpen: boolean;
  onClose: () => void;
}

export function CheatManager({ game, isOpen, onClose }: CheatManagerProps) {
  const [cheats, setCheats] = useState<CheatCode[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [showAddModal, setShowAddModal] = useState(false);
  const [showImportModal, setShowImportModal] = useState(false);
  const [editingCheat, setEditingCheat] = useState<CheatCode | null>(null);
  const [importText, setImportText] = useState('');

  // Form state for add/edit
  const [formName, setFormName] = useState('');
  const [formDescription, setFormDescription] = useState('');
  const [formCode, setFormCode] = useState('');
  const [formFormat, setFormFormat] = useState<CheatCodeFormat>('gamegenie');
  const [formCategory, setFormCategory] = useState<CheatCategory>('other');

  // Tracks the currently-open game id so async cheat loads can detect
  // whether the user has switched to a different game before the
  // IndexedDB read resolved (a ref avoids stale closures over state).
  const currentGameIdRef = useRef<string | null>(null);
  useEffect(() => {
    currentGameIdRef.current = game?.id ?? null;
  }, [game]);

  // Load cheats when game changes
  useEffect(() => {
    if (game && isOpen) {
      loadCheats(game.id);
    }
  }, [game, isOpen]);

  const loadCheats = async (gameId: string) => {
    setIsLoading(true);
    try {
      const db = await openDatabase();
      const transaction = db.transaction(STORE_NAME, 'readonly');
      const store = transaction.objectStore(STORE_NAME);
      const request = store.get(gameId);

      request.onsuccess = () => {
        // Discard stale results: if the user switched to another game
        // while this read was in flight, don't apply it.
        if (currentGameIdRef.current !== gameId) return;
        const data = request.result;
        setCheats(data?.cheats || []);
        setIsLoading(false);
      };

      request.onerror = () => {
        if (currentGameIdRef.current !== gameId) return;
        console.error('Failed to load cheats');
        setCheats([]);
        setIsLoading(false);
      };
    } catch (error) {
      if (currentGameIdRef.current !== gameId) return;
      console.error('Failed to open cheats database:', error);
      setCheats([]);
      setIsLoading(false);
    }
  };

  const saveCheats = async (gameId: string, newCheats: CheatCode[]) => {
    try {
      const db = await openDatabase();
      const transaction = db.transaction(STORE_NAME, 'readwrite');
      const store = transaction.objectStore(STORE_NAME);
      store.put({ gameId, cheats: newCheats });
    } catch (error) {
      console.error('Failed to save cheats:', error);
    }
  };

  async function openDatabase(): Promise<IDBDatabase> {
    return new Promise((resolve, reject) => {
      const request = indexedDB.open(DB_NAME, 1);

      request.onerror = () => reject(request.error);
      request.onsuccess = () => resolve(request.result);

      request.onupgradeneeded = (event) => {
        const db = (event.target as IDBOpenDBRequest).result;
        if (!db.objectStoreNames.contains(STORE_NAME)) {
          const store = db.createObjectStore(STORE_NAME, { keyPath: 'gameId' });
          store.createIndex('gameId', 'gameId', { unique: true });
        }
      };
    });
  }

  const handleToggleCheat = async (cheatId: string) => {
    const updatedCheats = cheats.map(c => 
      c.id === cheatId ? { ...c, enabled: !c.enabled } : c
    );
    setCheats(updatedCheats);
    if (game) {
      await saveCheats(game.id, updatedCheats);
    }
  };

  const handleDeleteCheat = async (cheatId: string) => {
    const updatedCheats = cheats.filter(c => c.id !== cheatId);
    setCheats(updatedCheats);
    if (game) {
      await saveCheats(game.id, updatedCheats);
    }
  };

  const handleAddCheat = async () => {
    if (!game || !formName.trim() || !formCode.trim()) return;

    const newCheat: CheatCode = {
      id: `cheat-${Date.now()}`,
      name: formName.trim(),
      description: formDescription.trim(),
      code: formCode.trim(),
      format: formFormat,
      enabled: false,
      gameId: game.id,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
    };

    const updatedCheats = [...cheats, newCheat];
    setCheats(updatedCheats);
    await saveCheats(game.id, updatedCheats);

    resetForm();
    setShowAddModal(false);
  };

  const handleEditCheat = async () => {
    if (!game || !editingCheat || !formName.trim() || !formCode.trim()) return;

    const updatedCheats = cheats.map(c => 
      c.id === editingCheat.id 
        ? { 
            ...c, 
            name: formName.trim(),
            description: formDescription.trim(),
            code: formCode.trim(),
            format: formFormat,
            updatedAt: new Date().toISOString(),
          } 
        : c
    );

    setCheats(updatedCheats);
    await saveCheats(game.id, updatedCheats);

    resetForm();
    setEditingCheat(null);
  };

  const resetForm = () => {
    setFormName('');
    setFormDescription('');
    setFormCode('');
    setFormFormat('gamegenie');
    setFormCategory('other');
  };

  const openEditModal = (cheat: CheatCode) => {
    setEditingCheat(cheat);
    setFormName(cheat.name);
    setFormDescription(cheat.description);
    setFormCode(cheat.code);
    setFormFormat(cheat.format);
  };

  const handleImport = async () => {
    if (!game || !importText.trim()) return;

    try {
      const data = JSON.parse(importText);
      
      // Handle single cheat or array of cheats
      let importedCheats: CheatCode[] = [];
      
      if (Array.isArray(data)) {
        importedCheats = data;
      } else if (data.cheats && Array.isArray(data.cheats)) {
        importedCheats = data.cheats;
      } else if (data.code) {
        importedCheats = [data as CheatCode];
      }

      // Assign new IDs and set gameId
      const newCheats = importedCheats.map(c => ({
        ...c,
        id: `cheat-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`,
        gameId: game.id,
        enabled: false,
        createdAt: new Date().toISOString(),
        updatedAt: new Date().toISOString(),
      }));

      const updatedCheats = [...cheats, ...newCheats];
      setCheats(updatedCheats);
      await saveCheats(game.id, updatedCheats);

      setImportText('');
      setShowImportModal(false);
    } catch (error) {
      console.error('Failed to import cheats:', error);
      alert('Invalid import format. Please check the JSON format.');
    }
  };

  const handleExport = () => {
    if (!game || cheats.length === 0) return;

    const exportData = {
      game: {
        id: game.id,
        title: game.title,
      },
      cheats: cheats,
      exportedAt: new Date().toISOString(),
    };

    const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `${game.file_name}_cheats.json`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const handleCodeFormatChange = (format: CheatCodeFormat) => {
    setFormFormat(format);
  };

  const formatOptions = [
    { value: 'gamegenie', label: 'Game Genie' },
    { value: 'proactionreplay', label: 'Pro Action Replay' },
    { value: 'goldfinger', label: 'Gold Finger' },
    { value: 'raw', label: 'Raw / Other' },
  ];

  const categoryOptions = Object.entries(CHEAT_CATEGORY_LABELS).map(([value, label]) => ({
    value,
    label,
  }));

  if (!isOpen) return null;

  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={`Cheats - ${game?.title || 'No game selected'}`}
      size="lg"
      footer={
        <>
          <Button variant="ghost" onClick={() => setShowImportModal(true)}>
            Import
          </Button>
          <Button variant="ghost" onClick={handleExport} disabled={cheats.length === 0}>
            Export
          </Button>
          <Button onClick={() => { resetForm(); setShowAddModal(true); }}>
            Add Cheat
          </Button>
        </>
      }
    >
      {!game ? (
        <p className="py-8 text-center text-mute">
          Select a game to manage its cheat codes.
        </p>
      ) : isLoading ? (
        <p className="py-8 text-center text-mute">
          Loading cheats...
        </p>
      ) : cheats.length === 0 ? (
        <div className="text-center py-8">
          <p className="mb-4 text-mute">No cheat codes for this game yet.</p>
          <Button onClick={() => { resetForm(); setShowAddModal(true); }}>
            Add Cheat Code
          </Button>
        </div>
      ) : (
        <div className="space-y-2 max-h-[400px] overflow-y-auto">
          {cheats.map((cheat) => (
            <div
              key={cheat.id}
              className={`flex items-center justify-between rounded-md border bg-raised p-3 ${
                cheat.enabled ? 'border-accent' : 'border-line'
              }`}
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-semibold text-ink">{cheat.name}</span>
                  <span className="chip">{cheat.format}</span>
                </div>
                <code className="mt-1 block truncate font-mono text-[0.75rem] text-dim">
                  {cheat.code}
                </code>
                {cheat.description && (
                  <p className="field-row-help">{cheat.description}</p>
                )}
              </div>
              <div className="flex items-center gap-2 ml-4">
                <Toggle
                  checked={cheat.enabled}
                  onChange={() => handleToggleCheat(cheat.id)}
                  size="sm"
                />
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => openEditModal(cheat)}
                >
                  Edit
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => handleDeleteCheat(cheat.id)}
                >
                  Delete
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}

      {/* Add Cheat Modal */}
      <Modal
        isOpen={showAddModal}
        onClose={() => { setShowAddModal(false); resetForm(); }}
        title="Add Cheat Code"
        footer={
          <>
            <Button variant="ghost" onClick={() => { setShowAddModal(false); resetForm(); }}>
              Cancel
            </Button>
            <Button 
              onClick={handleAddCheat}
              disabled={!formName.trim() || !formCode.trim()}
            >
              Add
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          <Input
            label="Name"
            value={formName}
            onChange={(e) => setFormName(e.target.value)}
            placeholder="e.g., Infinite Lives"
          />
          <TextArea
            label="Description (optional)"
            value={formDescription}
            onChange={(e) => setFormDescription(e.target.value)}
            placeholder="e.g., Never lose a life"
            rows={2}
          />
          <div className="grid grid-cols-2 gap-4">
            <Select
              label="Format"
              value={formFormat}
              onChange={(e) => handleCodeFormatChange(e.target.value as CheatCodeFormat)}
              options={formatOptions}
            />
            <Select
              label="Category"
              value={formCategory}
              onChange={(e) => setFormCategory(e.target.value as CheatCategory)}
              options={categoryOptions}
            />
          </div>
          <Input
            label="Cheat Code"
            value={formCode}
            onChange={(e) => setFormCode(e.target.value.toUpperCase())}
            placeholder="Enter cheat code"
            error={formCode && !validateCheatCode(formCode, formFormat) ? 'Invalid code format' : undefined}
          />
        </div>
      </Modal>

      {/* Edit Cheat Modal */}
      <Modal
        isOpen={!!editingCheat}
        onClose={() => { setEditingCheat(null); resetForm(); }}
        title="Edit Cheat Code"
        footer={
          <>
            <Button variant="ghost" onClick={() => { setEditingCheat(null); resetForm(); }}>
              Cancel
            </Button>
            <Button 
              onClick={handleEditCheat}
              disabled={!formName.trim() || !formCode.trim()}
            >
              Save
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          <Input
            label="Name"
            value={formName}
            onChange={(e) => setFormName(e.target.value)}
            placeholder="e.g., Infinite Lives"
          />
          <TextArea
            label="Description (optional)"
            value={formDescription}
            onChange={(e) => setFormDescription(e.target.value)}
            placeholder="e.g., Never lose a life"
            rows={2}
          />
          <div className="grid grid-cols-2 gap-4">
            <Select
              label="Format"
              value={formFormat}
              onChange={(e) => handleCodeFormatChange(e.target.value as CheatCodeFormat)}
              options={formatOptions}
            />
            <Select
              label="Category"
              value={formCategory}
              onChange={(e) => setFormCategory(e.target.value as CheatCategory)}
              options={categoryOptions}
            />
          </div>
          <Input
            label="Cheat Code"
            value={formCode}
            onChange={(e) => setFormCode(e.target.value.toUpperCase())}
            placeholder="Enter cheat code"
            error={formCode && !validateCheatCode(formCode, formFormat) ? 'Invalid code format' : undefined}
          />
        </div>
      </Modal>

      {/* Import Modal */}
      <Modal
        isOpen={showImportModal}
        onClose={() => { setShowImportModal(false); setImportText(''); }}
        title="Import Cheat Codes"
        footer={
          <>
            <Button variant="ghost" onClick={() => { setShowImportModal(false); setImportText(''); }}>
              Cancel
            </Button>
            <Button 
              onClick={handleImport}
              disabled={!importText.trim()}
            >
              Import
            </Button>
          </>
        }
      >
        <div className="space-y-4">
          <p className="text-sm text-dim">
            Paste JSON containing cheat codes — a single cheat, an array, or a
            database export.
          </p>
          <TextArea
            value={importText}
            onChange={(e) => setImportText(e.target.value)}
            placeholder='[{"name": "Infinite Lives", "code": "ABCD-EFGH", "format": "gamegenie"}]'
            rows={6}
          />
        </div>
      </Modal>
    </Modal>
  );
}
