import { useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import { useSettingsStore } from '../../stores/settingsStore';
import { useLibraryStore } from '../../stores/libraryStore';
import { Toggle } from '../common/Toggle';
import { Button } from '../common/Button';
import { ConfirmModal } from '../common/Modal';
import { IconFolder, IconClose, IconPlus } from '../common/icons';
import { SettingsSection, SettingRow, SettingNote } from './SettingsSection';
import {
  fetchCovers,
  gamesNeedingCovers,
  type FetchCoversProgress,
} from '../../domain/coverArt';

interface VerifyResult {
  removed_count: number;
  removed_titles: string[];
}

// The option lists for "artwork source" and "cover resolution" lived here. They
// are gone with the controls that used them: no code path read either value.

type Status = { tone: 'ok' | 'err'; text: string } | null;

/** Cover-art acquisition, with progress and a cache reset. */
function CoverArtSection() {
  const { settings } = useSettingsStore();
  const { games, loadGames } = useLibraryStore();

  const [progress, setProgress] = useState<FetchCoversProgress | null>(null);
  const [status, setStatus] = useState<Status>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  // A ref, not state: the workers read it on every iteration and must see the
  // latest value without waiting for a re-render.
  const cancelRef = useRef(false);

  const allowDownload = settings.library?.use_metadata !== false;
  const withCovers = games.filter((g) => g.cover_file || g.custom_cover_path).length;
  const missing = gamesNeedingCovers(games);

  const run = async (force: boolean) => {
    const targets = force ? games : missing;
    if (targets.length === 0) return;

    cancelRef.current = false;
    setStatus(null);
    setProgress({ done: 0, total: targets.length, found: 0, current: '' });
    try {
      const results = await fetchCovers(targets, {
        allowDownload,
        force,
        onProgress: setProgress,
        shouldStop: () => cancelRef.current,
      });
      await loadGames();

      const found = results.filter((r) => r.file).length;
      const unavailable = results.filter((r) => r.source === 'unavailable').length;
      setStatus({
        tone: unavailable > 0 && found === 0 ? 'err' : 'ok',
        text: cancelRef.current
          ? `Stopped. ${found} cover${found === 1 ? '' : 's'} added.`
          : unavailable > 0
            ? `${found} added, ${unavailable} could not be reached — check your connection and try again.`
            : `${found} cover${found === 1 ? '' : 's'} added. ${
                results.length - found
              } had no art available.`,
      });
    } catch (error) {
      console.error('Cover fetch failed:', error);
      setStatus({ tone: 'err', text: 'Cover lookup failed.' });
    } finally {
      setProgress(null);
    }
  };

  const handleClear = async () => {
    setConfirmClear(false);
    try {
      const removed = await invoke<number>('clear_cover_cache');
      await loadGames();
      setStatus({ tone: 'ok', text: `Removed ${removed} cached file(s).` });
    } catch (error) {
      console.error('Failed to clear cover cache:', error);
      setStatus({ tone: 'err', text: 'Could not clear the cover cache.' });
    }
  };

  return (
    <SettingsSection
      eyebrow="ARTWORK"
      title="Cover art"
      description="Games without a cover are drawn as cartridge labels, tinted from the title."
    >
      <SettingRow label="Covers found" help="Out of every game in your library.">
        <span className="register text-ink">
          {withCovers} / {games.length}
        </span>
      </SettingRow>

      <SettingRow
        label="Get cover art"
        help={
          allowDownload
            ? 'Checks for images beside your ROMs first, then the Libretro thumbnail archive.'
            : 'Only checks for images beside your ROMs — online lookup is off in Scanning.'
        }
      >
        {progress ? (
          <>
            <span className="register text-ink">
              {progress.done}/{progress.total}
            </span>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => {
                cancelRef.current = true;
              }}
            >
              Stop
            </Button>
          </>
        ) : (
          <Button size="sm" onClick={() => run(false)} disabled={missing.length === 0}>
            {missing.length === 0
              ? 'All covered'
              : `Fetch ${missing.length} missing`}
          </Button>
        )}
      </SettingRow>

      <SettingRow
        label="Re-check everything"
        help="Look again for every game, including ones already covered and ones previously found to have no art."
      >
        <Button
          variant="secondary"
          size="sm"
          onClick={() => run(true)}
          disabled={progress !== null || games.length === 0}
        >
          Re-check all
        </Button>
      </SettingRow>

      <SettingRow label="Cached images" help="Delete every downloaded cover and start over.">
        <Button
          variant="danger"
          size="sm"
          onClick={() => setConfirmClear(true)}
          disabled={progress !== null}
        >
          Clear cache
        </Button>
      </SettingRow>

      {progress?.current && (
        <SettingNote>
          Looking up <span className="text-ink">{progress.current}</span>…
        </SettingNote>
      )}

      {status && (
        <SettingNote tone={status.tone === 'err' ? 'danger' : 'accent'}>
          {status.text}
        </SettingNote>
      )}

      <SettingNote title="Where the art comes from">
        Box art is matched by file name against the Libretro thumbnail archive —
        the same public source RetroArch uses, with no account or API key. That
        means a ROM named the standard way (<span className="text-ink">Super
        Metroid (USA).sfc</span>) will match, and a renamed one may not. Drop your
        own image next to the ROM, or in a <span className="text-ink">covers</span>{' '}
        folder beside it, and that wins over any download.
      </SettingNote>

      <ConfirmModal
        isOpen={confirmClear}
        onClose={() => setConfirmClear(false)}
        onConfirm={handleClear}
        title="Clear cached covers?"
        message="Every downloaded image is deleted and all games go back to cartridge labels. Images you placed next to your ROMs are untouched, and a new fetch will pick them up again."
        confirmText="Clear cache"
        variant="danger"
      />
    </SettingsSection>
  );
}

export function LibrarySettings() {
  const { settings, updateSection } = useSettingsStore();
  const { loadGames } = useLibraryStore();
  const library = settings.library;
  const folders = library.folders || [];

  const [busy, setBusy] = useState<null | 'scan' | 'rescan' | 'verify'>(null);
  const [status, setStatus] = useState<Status>(null);
  const [confirmClear, setConfirmClear] = useState(false);
  const [pendingRemoval, setPendingRemoval] = useState<string | null>(null);

  const patchLibrary = (patch: Partial<typeof library>) => updateSection('library', patch);

  const handleAddFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: false,
      title: 'Select ROM folder',
    });
    if (typeof selected !== 'string') return;
    if (folders.includes(selected)) {
      setStatus({ tone: 'err', text: 'That folder is already in the list.' });
      return;
    }
    await patchLibrary({ folders: [...folders, selected] });
    setStatus(null);
  };

  const handleScan = async () => {
    setBusy('scan');
    setStatus(null);
    try {
      let total = 0;
      for (const path of folders) {
        // `scan_directory` only reports what it finds; `add_game_folder` is the
        // one that persists into library.json.
        const result = await invoke<{ total: number }>('add_game_folder', {
          path,
          recursive: library.scan_recursive !== false,
        });
        total = result.total;
      }
      await loadGames();
      setStatus({ tone: 'ok', text: `Scan complete. Library holds ${total} games.` });
    } catch (error) {
      console.error('Failed to scan folders:', error);
      setStatus({ tone: 'err', text: 'Scan failed. Check that the folders still exist.' });
    } finally {
      setBusy(null);
    }
  };

  const handleRescan = async () => {
    setBusy('rescan');
    setStatus(null);
    try {
      const result = await invoke<{ total: number }>('rescan_library');
      await loadGames();
      setStatus({ tone: 'ok', text: `Rescan complete. Library holds ${result.total} games.` });
    } catch (error) {
      console.error('Failed to rescan library:', error);
      setStatus({ tone: 'err', text: 'Rescan failed.' });
    } finally {
      setBusy(null);
    }
  };

  const handleVerify = async () => {
    setBusy('verify');
    setStatus(null);
    try {
      const result = await invoke<VerifyResult>('verify_library');
      await loadGames();
      setStatus({
        tone: 'ok',
        text:
          result.removed_count === 0
            ? 'Every game in the library is still on disk.'
            : `Removed ${result.removed_count} missing ${
                result.removed_count === 1 ? 'entry' : 'entries'
              }: ${result.removed_titles.join(', ')}`,
      });
    } catch (error) {
      console.error('Failed to verify library:', error);
      setStatus({ tone: 'err', text: 'Verification failed.' });
    } finally {
      setBusy(null);
    }
  };

  const handleClear = async () => {
    setConfirmClear(false);
    try {
      await invoke('clear_library');
      await loadGames();
      setStatus({ tone: 'ok', text: 'Library cleared. Your ROM files were not touched.' });
    } catch (error) {
      console.error('Failed to clear library:', error);
      setStatus({ tone: 'err', text: 'Could not clear the library.' });
    }
  };

  return (
    <div className="space-y-4">
      <SettingsSection
        eyebrow="SOURCES"
        title="ROM folders"
        description="Folders searched for cartridge images. Nothing is copied or moved — the library only records where each file is."
        action={
          <Button variant="secondary" size="sm" leftIcon={<IconPlus size={15} />} onClick={handleAddFolder}>
            Add folder
          </Button>
        }
      >
        {folders.length === 0 ? (
          <SettingNote>
            No folders yet. Add one and run a scan to fill the library.
          </SettingNote>
        ) : (
          <ul className="mt-3 space-y-1.5">
            {folders.map((path) => (
              <li
                key={path}
                className="flex items-center gap-2.5 rounded-md border border-line bg-raised px-3 py-2"
              >
                <span className="flex-none text-mute">
                  <IconFolder size={16} />
                </span>
                <span
                  className="min-w-0 flex-1 truncate font-mono text-[0.75rem] text-ink"
                  title={path}
                >
                  {path}
                </span>
                <button
                  type="button"
                  onClick={() => setPendingRemoval(path)}
                  className="btn btn--ghost h-7 w-7 flex-none p-0"
                  title={`Stop watching ${path}`}
                  aria-label={`Stop watching ${path}`}
                >
                  <IconClose size={14} />
                </button>
              </li>
            ))}
          </ul>
        )}

        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            size="sm"
            onClick={handleScan}
            isLoading={busy === 'scan'}
            disabled={busy !== null || folders.length === 0}
          >
            Scan folders
          </Button>
          <Button
            variant="secondary"
            size="sm"
            onClick={handleRescan}
            isLoading={busy === 'rescan'}
            disabled={busy !== null}
          >
            Rescan everything
          </Button>
        </div>

        {status && (
          <SettingNote tone={status.tone === 'err' ? 'danger' : 'accent'}>
            {status.text}
          </SettingNote>
        )}
      </SettingsSection>

      <SettingsSection eyebrow="SCANNING" title="How folders are read">
        <SettingRow
          label="Include subfolders"
          help="Descend into nested folders when scanning. Turn this off if a ROM folder sits inside a much larger tree."
        >
          <Toggle
            checked={library.scan_recursive !== false}
            onChange={(e) => patchLibrary({ scan_recursive: e.target.checked })}
            aria-label="Include subfolders"
          />
        </SettingRow>

        <SettingRow
          label="Fetch metadata"
          help="Look up titles and artwork online for newly found games."
        >
          <Toggle
            checked={library.use_metadata !== false}
            onChange={(e) => patchLibrary({ use_metadata: e.target.checked })}
            aria-label="Fetch metadata"
          />
        </SettingRow>
      </SettingsSection>

      {/*
        The "Artwork source" and "Cover image resolution" dropdowns that used to
        sit here are gone. Source offered ScreenScraper / IGDB / OpenVGDB, none of
        which can run without credentials this app cannot ship, and resolution
        offered four pixel sizes that no code path ever read -- the Libretro
        archive serves one image per game. Both were settings that saved a value
        and changed nothing. The fields remain in `LibrarySettings` so existing
        settings.json files keep loading, ready for a credentialed tier later.
      */}
      <CoverArtSection />

      <SettingsSection
        eyebrow="MAINTENANCE"
        title="Library data"
        description="Titles, play counts, favourites and folder assignments. Your ROM files are never modified by anything here."
      >
        <SettingRow
          label="Verify"
          help="Drop entries whose file is no longer on disk."
        >
          <Button
            variant="secondary"
            size="sm"
            onClick={handleVerify}
            isLoading={busy === 'verify'}
            disabled={busy !== null}
          >
            Verify library
          </Button>
        </SettingRow>

        <SettingRow
          label="Clear"
          help="Remove every entry and start over. ROM files stay where they are."
        >
          <Button variant="danger" size="sm" onClick={() => setConfirmClear(true)}>
            Clear library
          </Button>
        </SettingRow>
      </SettingsSection>

      <ConfirmModal
        isOpen={confirmClear}
        onClose={() => setConfirmClear(false)}
        onConfirm={handleClear}
        title="Clear the library?"
        message="Every entry goes, along with play counts and favourites. Your ROM files are not deleted, and a scan will find them again — but play history cannot be recovered."
        confirmText="Clear library"
        variant="danger"
      />

      <ConfirmModal
        isOpen={pendingRemoval !== null}
        onClose={() => setPendingRemoval(null)}
        onConfirm={() => {
          if (pendingRemoval) {
            void patchLibrary({ folders: folders.filter((p) => p !== pendingRemoval) });
          }
          setPendingRemoval(null);
        }}
        title="Stop watching this folder?"
        message="Future scans will skip it. Games already in your library stay there until you verify or clear it."
        confirmText="Stop watching"
      />
    </div>
  );
}
