import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { IconLibrary, IconStar, IconClock } from '../common/icons';

/** Quick views: predefined slices of the library. */
export type QuickView = 'all' | 'favorites' | 'recent';

export interface FilterState {
  quickView: QuickView;
  /** Selected region values, matching `Game.country` lowercased. Empty = any. */
  regions: string[];
}

export const EMPTY_FILTERS: FilterState = { quickView: 'all', regions: [] };

interface FilterSidebarProps {
  filters: FilterState;
  onChange: (filters: FilterState) => void;
  /** Total games, for the "All games" count. */
  totalCount: number;
  favoriteCount: number;
  /** Bumped by the library after a scan so the region counts refresh. */
  refreshKey?: number;
}

/**
 * A region facet, derived from what the library actually contains.
 *
 * Deliberately not a hardcoded list. `Country::as_str()` (src-tauri/src/rom/
 * header.rs) can return 16 different strings — Scandinavia, France, Germany,
 * Italy, Spain, Netherlands, Belgium, United Kingdom, Canada, Australia, Other
 * and Unknown among them — and an earlier hardcoded four (USA/Japan/Europe/
 * Brazil) left every other region unreachable even though `get_filter_counts`
 * was counting it. A German or French PAL collection matched none of the four
 * and got a facet with nothing selectable in it.
 *
 * Building the list from the counts also removes the need to decide what to do
 * with empty regions: a region that no game reports simply is not a facet.
 */
interface RegionFacet {
  /** Lowercased `Game.country`, which is what the filter compares against. */
  value: string;
  /** The backend's own spelling, used as the visible label. */
  label: string;
  count: number;
}

const QUICK_VIEWS: Array<{ value: QuickView; label: string; icon: React.ReactNode }> = [
  { value: 'all', label: 'All games', icon: <IconLibrary size={16} /> },
  { value: 'favorites', label: 'Favourites', icon: <IconStar size={16} /> },
  { value: 'recent', label: 'Recently played', icon: <IconClock size={16} /> },
];

/**
 * Library facets.
 *
 * Deliberately narrow: quick views and region only. Search and sort used to live
 * here as well, duplicating the toolbar's own search field and sort menu — two
 * controls for the same state, either of which could contradict the other. Those
 * belong to the toolbar because they act on what you are looking at; a facet
 * chooses *which set* you are looking at.
 */
export function FilterSidebar({
  filters,
  onChange,
  totalCount,
  favoriteCount,
  refreshKey = 0,
}: FilterSidebarProps) {
  const [regions, setRegions] = useState<RegionFacet[]>([]);

  useEffect(() => {
    invoke<{ regions: Record<string, number> }>('get_filter_counts')
      .then((counts) => {
        const facets = Object.entries(counts.regions || {})
          .filter(([, count]) => count > 0)
          .map(([label, count]) => ({ value: label.toLowerCase(), label, count }))
          // Biggest first, so the regions worth filtering by are at the top of a
          // mixed collection; alphabetical within a tie for a stable order.
          .sort((a, b) => b.count - a.count || a.label.localeCompare(b.label));
        setRegions(facets);
      })
      .catch((error) => {
        console.error('Failed to load filter counts:', error);
        setRegions([]);
      });
  }, [refreshKey]);

  const toggleRegion = (value: string) => {
    const regions = filters.regions.includes(value)
      ? filters.regions.filter((r) => r !== value)
      : [...filters.regions, value];
    onChange({ ...filters, regions });
  };

  const quickViewCount = (view: QuickView) =>
    view === 'favorites' ? favoriteCount : view === 'all' ? totalCount : null;

  return (
    <div className="space-y-5">
      <div>
        <p className="eyebrow mb-2 px-1">Views</p>
        <ul className="space-y-0.5">
          {QUICK_VIEWS.map((view) => {
            const active = filters.quickView === view.value;
            const count = quickViewCount(view.value);
            return (
              <li key={view.value}>
                <button
                  type="button"
                  onClick={() => onChange({ ...filters, quickView: view.value })}
                  aria-current={active ? 'true' : undefined}
                  className={`flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-[0.8125rem] font-semibold transition-colors ${
                    active
                      ? 'bg-accent-soft text-accent-text'
                      : 'text-dim hover:bg-raised hover:text-ink'
                  }`}
                >
                  <span className="flex-none">{view.icon}</span>
                  <span className="min-w-0 flex-1 truncate">{view.label}</span>
                  {count !== null && <span className="register flex-none">{count}</span>}
                </button>
              </li>
            );
          })}
        </ul>
      </div>

      <div>
        <div className="mb-2 flex items-center justify-between px-1">
          <p className="eyebrow">Region</p>
          {filters.regions.length > 0 && (
            <button
              type="button"
              onClick={() => onChange({ ...filters, regions: [] })}
              className="register hover:text-ink"
            >
              clear
            </button>
          )}
        </div>
        {regions.length === 0 ? (
          <p className="px-2 text-[0.8125rem] leading-relaxed text-mute">
            Regions appear here once your library has games to group.
          </p>
        ) : (
          <ul className="space-y-0.5">
            {regions.map((region) => {
              const checked = filters.regions.includes(region.value);
              return (
                <li key={region.value}>
                  <label
                    className={`flex cursor-pointer items-center gap-2.5 rounded-md px-2 py-1.5 text-[0.8125rem] transition-colors ${
                      checked
                        ? 'bg-accent-soft text-accent-text'
                        : 'text-dim hover:bg-raised hover:text-ink'
                    }`}
                  >
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => toggleRegion(region.value)}
                      className="h-3.5 w-3.5 flex-none rounded border-line accent-[var(--accent-solid)]"
                    />
                    <span className="min-w-0 flex-1 truncate font-semibold">{region.label}</span>
                    <span className="register flex-none">{region.count}</span>
                  </label>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
