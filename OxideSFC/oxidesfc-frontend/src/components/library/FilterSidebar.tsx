import { useState, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Button } from '../common/Button';
import { Input } from '../common/Input';
import { Select } from '../common/Select';

interface FilterSidebarProps {
  theme?: 'dark' | 'light';
  onFilterChange?: (filters: FilterState) => void;
  initialFilters?: FilterState;
}

export interface FilterState {
  searchQuery: string;
  systems: string[];
  regions: string[];
  genres: string[];
  sortBy: 'title' | 'date_added' | 'last_played' | 'play_count';
  sortOrder: 'asc' | 'desc';
}

interface FilterOption {
  value: string;
  label: string;
  count?: number;
}

// Available systems
const SYSTEMS: FilterOption[] = [
  { value: 'snes', label: 'Super Nintendo' },
  { value: 'sfc', label: 'Super Famicom' },
];

// Available regions. `value` matches the lowercased form of the backend's
// `Game.country` string (see `rom::header::Country::as_str()` in
// src-tauri/src/rom/header.rs -- "USA", "Japan", "Europe", "Brazil", etc.)
// so counts from `get_filter_counts` can be looked up case-insensitively
// below. "Korea"/"International" have no corresponding `Country` variant on
// the backend today, so their counts will simply never show a number --
// that's expected, not a bug.
const REGIONS: FilterOption[] = [
  { value: 'usa', label: 'USA' },
  { value: 'europe', label: 'Europe' },
  { value: 'japan', label: 'Japan' },
  { value: 'korea', label: 'Korea' },
  { value: 'brazil', label: 'Brazil' },
  { value: 'international', label: 'International' },
];

// Sort options
const SORT_OPTIONS: FilterOption[] = [
  { value: 'title', label: 'Title' },
  { value: 'date_added', label: 'Date Added' },
  { value: 'last_played', label: 'Last Played' },
  { value: 'play_count', label: 'Play Count' },
];

export function FilterSidebar({
  theme = 'dark',
  onFilterChange,
  initialFilters,
}: FilterSidebarProps) {
  const [filters, setFilters] = useState<FilterState>(initialFilters || {
    searchQuery: '',
    systems: [],
    regions: [],
    genres: [],
    sortBy: 'title',
    sortOrder: 'asc',
  });

  const [regionCounts, setRegionCounts] = useState<Record<string, number>>({});

  useEffect(() => {
    loadFilterCounts();
  }, []);

  useEffect(() => {
    onFilterChange?.(filters);
  }, [filters, onFilterChange]);

  const loadFilterCounts = async () => {
    try {
      // Backend only tracks region counts (`Game.country`) -- there is no
      // genre field anywhere in the ROM header parsing or the `Game`
      // struct, so no genre counts exist to load. Region keys come back
      // capitalized (e.g. "USA", "Japan") to match `Country::as_str()`;
      // normalize to lowercase here so they can be looked up by this file's
      // lowercase `REGIONS[].value`s.
      const counts = await invoke<{ regions: Record<string, number> }>('get_filter_counts');
      const normalized: Record<string, number> = {};
      for (const [region, count] of Object.entries(counts.regions || {})) {
        normalized[region.toLowerCase()] = count;
      }
      setRegionCounts(normalized);
    } catch (error) {
      console.error('Failed to load filter counts:', error);
    }
  };

  const handleSearchChange = (value: string) => {
    setFilters(prev => ({ ...prev, searchQuery: value }));
  };

  const handleSortByChange = (value: string) => {
    setFilters(prev => ({ 
      ...prev, 
      sortBy: value as FilterState['sortBy'] 
    }));
  };

  const handleSortOrderToggle = () => {
    setFilters(prev => ({ 
      ...prev, 
      sortOrder: prev.sortOrder === 'asc' ? 'desc' : 'asc' 
    }));
  };

  const handleToggleFilter = (
    category: 'systems' | 'regions' | 'genres',
    value: string
  ) => {
    setFilters(prev => {
      const current = prev[category];
      const updated = current.includes(value)
        ? current.filter(v => v !== value)
        : [...current, value];
      return { ...prev, [category]: updated };
    });
  };

  const handleClearFilters = () => {
    setFilters({
      searchQuery: '',
      systems: [],
      regions: [],
      genres: [],
      sortBy: 'title',
      sortOrder: 'asc',
    });
  };

  const hasActiveFilters = 
    filters.systems.length > 0 ||
    filters.regions.length > 0 ||
    filters.genres.length > 0 ||
    filters.searchQuery.length > 0;

  const containerClass = theme === 'light'
    ? 'bg-white border-gray-200'
    : 'bg-slate-800 border-slate-700';

  const textClass = theme === 'light'
    ? 'text-gray-700'
    : 'text-slate-200';

  const mutedClass = theme === 'light'
    ? 'text-gray-500'
    : 'text-slate-400';

  const inputClass = theme === 'light'
    ? 'bg-gray-100 border-gray-300'
    : 'bg-slate-700 border-slate-600';

  const checkboxClass = theme === 'light'
    ? 'border-gray-300 bg-white'
    : 'border-slate-500 bg-slate-700';

  return (
    <div className={`h-full flex flex-col rounded-lg border ${containerClass}`}>
      {/* Header */}
      <div className={`p-4 border-b ${theme === 'light' ? 'border-gray-200' : 'border-slate-700'}`}>
        <div className="flex items-center justify-between">
          <h2 className={`font-semibold ${textClass}`}>Filters</h2>
          {hasActiveFilters && (
            <Button
              variant="ghost"
              size="sm"
              onClick={handleClearFilters}
            >
              Clear All
            </Button>
          )}
        </div>
      </div>

      {/* Search */}
      <div className="p-4 border-b border-slate-700">
        <div className="relative">
          <Input
            label="Search"
            value={filters.searchQuery}
            onChange={(e) => handleSearchChange(e.target.value)}
            placeholder="Search games..."
            className={inputClass}
          />
          <svg 
            className={`absolute right-3 top-9 w-4 h-4 ${mutedClass}`} 
            fill="none" 
            viewBox="0 0 24 24" 
            stroke="currentColor"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
          </svg>
        </div>
      </div>

      {/* Sort Options */}
      <div className="p-4 border-b border-slate-700">
        <h3 className={`text-sm font-medium mb-3 ${textClass}`}>Sort By</h3>
        <div className="space-y-2">
          <Select
            value={filters.sortBy}
            onChange={(e) => handleSortByChange(e.target.value)}
            options={SORT_OPTIONS}
          />
          <Button
            variant="ghost"
            size="sm"
            onClick={handleSortOrderToggle}
            className="w-full"
          >
            <span className="flex items-center justify-center gap-2">
              {filters.sortOrder === 'asc' ? '↑ Ascending' : '↓ Descending'}
            </span>
          </Button>
        </div>
      </div>

      {/* System Filter */}
      <div className="p-4 border-b border-slate-700">
        <h3 className={`text-sm font-medium mb-3 ${textClass}`}>System</h3>
        <div className="space-y-2">
          {SYSTEMS.map((system) => (
            <label
              key={system.value}
              className={`flex items-center gap-2 cursor-pointer ${
                filters.systems.includes(system.value) ? 'text-primary-500' : textClass
              }`}
            >
              <input
                type="checkbox"
                checked={filters.systems.includes(system.value)}
                onChange={() => handleToggleFilter('systems', system.value)}
                className={`rounded ${checkboxClass}`}
              />
              <span className="flex-1 text-sm">{system.label}</span>
            </label>
          ))}
        </div>
      </div>

      {/* Region Filter */}
      <div className="p-4 border-b border-slate-700">
        <h3 className={`text-sm font-medium mb-3 ${textClass}`}>Region</h3>
        <div className="space-y-2 max-h-40 overflow-auto">
          {REGIONS.map((region) => (
            <label
              key={region.value}
              className={`flex items-center gap-2 cursor-pointer ${
                filters.regions.includes(region.value) ? 'text-primary-500' : textClass
              }`}
            >
              <input
                type="checkbox"
                checked={filters.regions.includes(region.value)}
                onChange={() => handleToggleFilter('regions', region.value)}
                className={`rounded ${checkboxClass}`}
              />
              <span className="flex-1 text-sm">{region.label}</span>
              {regionCounts[region.value] !== undefined && (
                <span className={`text-xs ${mutedClass}`}>
                  ({regionCounts[region.value]})
                </span>
              )}
            </label>
          ))}
        </div>
      </div>

      {/* Genre Filter intentionally omitted: there is no genre field
          anywhere in `Game` or the ROM header parsing (see
          `get_filter_counts` in src-tauri/src/commands/library.rs), so a
          genre section here would only ever show checkboxes with no counts
          behind them. `FilterState.genres` is kept (always empty) so
          existing consumers of `onFilterChange` don't need shape changes. */}
    </div>
  );
}
