import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useSettingsStore } from '../../stores/settingsStore';
import { Toggle } from '../common/Toggle';
import { Button } from '../common/Button';
import { Select } from '../common/Select';

interface VerifyResult {
  removed_count: number;
  removed_titles: string[];
}

// Artwork source options
const ARTWORK_SOURCE_OPTIONS = [
  { value: 'local', label: 'Local Files' },
  { value: 'screenscraper', label: 'ScreenScraper' },
  { value: 'igdb', label: 'IGDB' },
  { value: 'openvgdb', label: 'OpenVGDB' },
];

// Cover resolution options
const COVER_RESOLUTION_OPTIONS = [
  { value: 'thumbnail', label: 'Thumbnail (150px)' },
  { value: 'small', label: 'Small (250px)' },
  { value: 'medium', label: 'Medium (350px)' },
  { value: 'large', label: 'Large (500px)' },
];

export function LibrarySettings() {
  const { settings, saveSettings } = useSettingsStore();
  const theme = (settings.general.theme as 'dark' | 'light') || 'dark';
  const library = settings.library;

  const [romPaths, setRomPaths] = useState<string[]>(library.folders || []);
  const [scanOnStartup, setScanOnStartup] = useState(library.scan_recursive !== false);
  const [useMetadata, setUseMetadata] = useState(library.use_metadata !== false);
  const [coverResolution, setCoverResolution] = useState(library.cover_resolution || 'medium');
  const [artworkSource, setArtworkSource] = useState(library.artwork_source || 'screenscraper');
  const [isScanning, setIsScanning] = useState(false);
  const [libraryMessage, setLibraryMessage] = useState<string | null>(null);

  useEffect(() => {
    setRomPaths(library.folders || []);
    setScanOnStartup(library.scan_recursive !== false);
    setUseMetadata(library.use_metadata !== false);
    setCoverResolution(library.cover_resolution || 'medium');
    setArtworkSource(library.artwork_source || 'screenscraper');
  }, [library]);

  const handleAddPath = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select ROM Folder',
    });

    if (selected && typeof selected === 'string') {
      if (!romPaths.includes(selected)) {
        const newPaths = [...romPaths, selected];
        setRomPaths(newPaths);
        await saveSettings({
          ...settings,
          library: {
            ...library,
            folders: newPaths,
          },
        });
      }
    }
  };

  const handleRemovePath = async (path: string) => {
    const newPaths = romPaths.filter(p => p !== path);
    setRomPaths(newPaths);
    await saveSettings({
      ...settings,
      library: {
        ...library,
        folders: newPaths,
      },
    });
  };

  const handleToggleScanOnStartup = async (enabled: boolean) => {
    setScanOnStartup(enabled);
    await saveSettings({
      ...settings,
      library: {
        ...library,
        scan_recursive: enabled,
      },
    });
  };

  const handleToggleUseMetadata = async (enabled: boolean) => {
    setUseMetadata(enabled);
    await saveSettings({
      ...settings,
      library: {
        ...library,
        use_metadata: enabled,
      },
    });
  };

  const handleCoverResolutionChange = async (value: string) => {
    setCoverResolution(value);
    await saveSettings({
      ...settings,
      library: {
        ...library,
        cover_resolution: value,
      },
    });
  };

  const handleArtworkSourceChange = async (value: string) => {
    setArtworkSource(value);
    await saveSettings({
      ...settings,
      library: {
        ...library,
        artwork_source: value,
      },
    });
  };

  const handleScanLibrary = async () => {
    setIsScanning(true);
    try {
      // `scan_directory` only scans and returns results in memory; it never
      // saves to library.json. `add_game_folder` scans and persists.
      for (const path of romPaths) {
        await invoke('add_game_folder', { path });
      }
    } catch (error) {
      console.error('Failed to scan library:', error);
    } finally {
      setIsScanning(false);
    }
  };

  const handleRescanAll = async () => {
    setIsScanning(true);
    setLibraryMessage(null);
    try {
      const result = await invoke<{ total: number }>('rescan_library');
      setLibraryMessage(`Rescan complete. Library now has ${result.total} game(s).`);
    } catch (error) {
      console.error('Failed to rescan library:', error);
      setLibraryMessage('Failed to rescan library.');
    } finally {
      setIsScanning(false);
    }
  };

  const handleVerifyLibrary = async () => {
    setLibraryMessage(null);
    try {
      const result = await invoke<VerifyResult>('verify_library');
      if (result.removed_count === 0) {
        setLibraryMessage('Verification complete. All games are present.');
      } else {
        setLibraryMessage(
          `Removed ${result.removed_count} missing game(s): ${result.removed_titles.join(', ')}`
        );
      }
    } catch (error) {
      console.error('Failed to verify library:', error);
      setLibraryMessage('Failed to verify library.');
    }
  };

  const handleClearLibrary = async () => {
    if (!confirm('Are you sure you want to clear the library? This will remove all game entries and metadata.')) {
      return;
    }
    try {
      await invoke('clear_library');
      setLibraryMessage('Library cleared.');
    } catch (error) {
      console.error('Failed to clear library:', error);
      setLibraryMessage('Failed to clear library.');
    }
  };

  const containerClass = theme === 'light'
    ? 'bg-white'
    : 'bg-slate-800';

  const textClass = theme === 'light'
    ? 'text-gray-700'
    : 'text-slate-200';

  const mutedClass = theme === 'light'
    ? 'text-gray-500'
    : 'text-slate-400';

  const pathItemClass = theme === 'light'
    ? 'bg-gray-100 hover:bg-gray-200'
    : 'bg-slate-700 hover:bg-slate-600';

  return (
    <div className="space-y-6">
      {/* ROM Paths */}
      <section className={`rounded-lg p-6 ${containerClass}`}>
        <div className="flex items-center justify-between mb-4">
          <h2 className="text-lg font-semibold">ROM Folders</h2>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleAddPath}
          >
            + Add Folder
          </Button>
        </div>

        {romPaths.length === 0 ? (
          <p className={`text-sm ${mutedClass}`}>
            No ROM folders configured. Add a folder to start scanning for games.
          </p>
        ) : (
          <div className="space-y-2">
            {romPaths.map((path) => (
              <div
                key={path}
                className={`flex items-center justify-between p-3 rounded-lg ${pathItemClass}`}
              >
                <span className={`text-sm truncate flex-1 mr-4 ${textClass}`}>
                  {path}
                </span>
                <button
                  onClick={() => handleRemovePath(path)}
                  className={`p-1 rounded ${theme === 'light' ? 'hover:bg-gray-300' : 'hover:bg-slate-500'}`}
                  title="Remove folder"
                >
                  ✕
                </button>
              </div>
            ))}
          </div>
        )}

        <div className="mt-4 flex gap-2">
          <Button
            variant="primary"
            size="sm"
            onClick={handleScanLibrary}
            disabled={isScanning || romPaths.length === 0}
          >
            {isScanning ? 'Scanning...' : 'Scan for Games'}
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleRescanAll}
            disabled={isScanning}
          >
            Rescan All
          </Button>
        </div>
      </section>

      {/* Scanning Options */}
      <section className={`rounded-lg p-6 ${containerClass}`}>
        <h2 className="text-lg font-semibold mb-4">Scanning Options</h2>
        
        <div className="space-y-4">
          <Toggle
            checked={scanOnStartup}
            onChange={(e) => handleToggleScanOnStartup(e.target.checked)}
            label="Scan for new games on startup"
            description="Automatically scan ROM folders when the application starts"
          />

          <Toggle
            checked={useMetadata}
            onChange={(e) => handleToggleUseMetadata(e.target.checked)}
            label="Automatic metadata fetching"
            description="Fetch game information and artwork from online databases"
          />
        </div>
      </section>

      {/* Artwork Settings */}
      <section className={`rounded-lg p-6 ${containerClass}`}>
        <h2 className="text-lg font-semibold mb-4">Artwork</h2>
        
        <div className="space-y-4">
          <Select
            label="Artwork Source"
            value={artworkSource}
            options={ARTWORK_SOURCE_OPTIONS}
            onChange={(e) => handleArtworkSourceChange(e.target.value)}
            helperText="Preferred source for downloading game artwork"
          />

          <Select
            label="Cover Image Resolution"
            value={coverResolution}
            options={COVER_RESOLUTION_OPTIONS}
            onChange={(e) => handleCoverResolutionChange(e.target.value)}
            helperText="Higher resolutions use more storage space"
          />
        </div>
      </section>

      {/* Library Management */}
      <section className={`rounded-lg p-6 ${containerClass}`}>
        <h2 className="text-lg font-semibold mb-4">Library Management</h2>
        
        <div className="space-y-4">
          <div className={`p-3 rounded-lg text-sm ${
            theme === 'light' ? 'bg-gray-100 text-gray-600' : 'bg-slate-700 text-slate-400'
          }`}>
            <div className="flex items-center gap-2 mb-2">
              <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 7v10c0 2.21 3.582 4 8 4s8-1.79 8-4V7M4 7c0 2.21 3.582 4 8 4s8-1.79 8-4M4 7c0-2.21 3.582-4 8-4s8 1.79 8 4" />
              </svg>
              <span className="font-medium">Database Location</span>
            </div>
            <p>Game library data is stored locally. The database includes game metadata, play statistics, and folder associations.</p>
          </div>

          {libraryMessage && (
            <div className={`p-3 rounded-lg text-sm ${
              theme === 'light' ? 'bg-blue-50 text-blue-700' : 'bg-blue-900/30 text-blue-300'
            }`}>
              {libraryMessage}
            </div>
          )}

          <div className="flex gap-2">
            <Button
              variant="secondary"
              size="sm"
              onClick={handleVerifyLibrary}
            >
              Verify Library
            </Button>
            <Button
              variant="danger"
              size="sm"
              onClick={handleClearLibrary}
            >
              Clear Library
            </Button>
          </div>
        </div>
      </section>
    </div>
  );
}
