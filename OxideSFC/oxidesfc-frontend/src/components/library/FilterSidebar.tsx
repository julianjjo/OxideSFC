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
 * Region values, matching the lowercased form of the backend's `Game.country`
 * string (see `Country::as_str()` in src-tauri/src/rom/header.rs) so counts from
 * `get_filter_counts` can be looked up case-insensitively.
 *
 * Only regions the backend can actually report are listed. The previous version
 * also offered Korea and International, which have no corresponding `Country`
 * variant -- they were permanently empty checkboxes that could only ever filter
 * the library down to nothing.
 */
const REGIONS: Array<{ value: string; label: string }> = [
  { value: 'usa', label: 'USA' },
  { value: 'japan', label: 'Japan' },
  { value: 'europe', label: 'Europe' },
  { value: 'brazil', label: 'Brazil' },
];

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
  const [regionCounts, setRegionCounts] = useState<Record<string, number>>({});

  useEffect(() => {
    invoke<{ regions: Record<string, number> }>('get_filter_counts')
      .then((counts) => {
        // Keys come back capitalised to match `Country::as_str()`; normalise so
        // they can be looked up by this file's lowercase values.
        const normalized: Record<string, number> = {};
        for (const [region, count] of Object.entries(counts.regions || {})) {
          normalized[region.toLowerCase()] = count;
        }
        setRegionCounts(normalized);
      })
      .catch((error) => {
        console.error('Failed to load filter counts:', error);
        setRegionCounts({});
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

  // Regions with nothing behind them are shown but disabled, so the set of
  // facets stays stable as the library grows instead of controls appearing and
  // vanishing between scans.
  const availableRegions = REGIONS.filter((r) => (regionCounts[r.value] ?? 0) > 0);
  const shownRegions = availableRegions.length > 0 ? availableRegions : REGIONS;

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
        <ul className="space-y-0.5">
          {shownRegions.map((region) => {
            const count = regionCounts[region.value] ?? 0;
            const checked = filters.regions.includes(region.value);
            return (
              <li key={region.value}>
                <label
                  className={`flex cursor-pointer items-center gap-2.5 rounded-md px-2 py-1.5 text-[0.8125rem] transition-colors ${
                    count === 0
                      ? 'cursor-not-allowed text-mute opacity-50'
                      : checked
                        ? 'bg-accent-soft text-accent-text'
                        : 'text-dim hover:bg-raised hover:text-ink'
                  }`}
                >
                  <input
                    type="checkbox"
                    checked={checked}
                    disabled={count === 0}
                    onChange={() => toggleRegion(region.value)}
                    className="h-3.5 w-3.5 flex-none rounded border-line accent-[var(--accent-solid)]"
                  />
                  <span className="min-w-0 flex-1 truncate font-semibold">{region.label}</span>
                  <span className="register flex-none">{count}</span>
                </label>
              </li>
            );
          })}
        </ul>
      </div>
    </div>
  );
}
