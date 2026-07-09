import React, { forwardRef, useState, useRef, useEffect } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface SelectProps extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'size'> {
  label?: string;
  error?: string;
  helperText?: string;
  options: SelectOption[];
  placeholder?: string;
  inputSize?: 'sm' | 'md' | 'lg';
  isSearchable?: boolean;
}

export const Select = forwardRef<HTMLSelectElement, SelectProps>(({
  label,
  error,
  helperText,
  options,
  placeholder = 'Select an option',
  inputSize = 'md',
  isSearchable = false,
  className = '',
  id,
  value,
  onChange,
  ...props
}, ref) => {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme;

  const [isOpen, setIsOpen] = useState(false);
  const [searchValue, setSearchValue] = useState('');
  const [highlightedIndex, setHighlightedIndex] = useState(0);
  const containerRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLUListElement>(null);

  const inputId = id || `select-${Math.random().toString(36).substr(2, 9)}`;

  const sizeStyles = {
    sm: 'px-2.5 py-1.5 text-sm',
    md: 'px-3 py-2 text-base',
    lg: 'px-4 py-3 text-lg',
  };

  const filteredOptions = isSearchable
    ? options.filter(option =>
        option.label.toLowerCase().includes(searchValue.toLowerCase())
      )
    : options;

  const selectedOption = options.find(opt => opt.value === value);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
        setSearchValue('');
      }
    };

    document.addEventListener('mousedown', handleClickOutside);
    return () => document.removeEventListener('mousedown', handleClickOutside);
  }, []);

  useEffect(() => {
    if (isOpen && listRef.current) {
      const highlightedElement = listRef.current.children[highlightedIndex] as HTMLElement;
      if (highlightedElement) {
        highlightedElement.scrollIntoView({ block: 'nearest' });
      }
    }
  }, [highlightedIndex, isOpen]);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (!isOpen) {
      if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown') {
        setIsOpen(true);
        e.preventDefault();
      }
      return;
    }

    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        setHighlightedIndex(prev =>
          prev < filteredOptions.length - 1 ? prev + 1 : 0
        );
        break;
      case 'ArrowUp':
        e.preventDefault();
        setHighlightedIndex(prev =>
          prev > 0 ? prev - 1 : filteredOptions.length - 1
        );
        break;
      case 'Enter':
      case ' ':
        e.preventDefault();
        if (filteredOptions[highlightedIndex] && !filteredOptions[highlightedIndex].disabled) {
          const event = {
            target: { value: filteredOptions[highlightedIndex].value }
          } as React.ChangeEvent<HTMLSelectElement>;
          onChange?.(event);
          setIsOpen(false);
          setSearchValue('');
        }
        break;
      case 'Escape':
        setIsOpen(false);
        setSearchValue('');
        break;
    }
  };

  const baseStyles = `w-full rounded-lg transition-colors focus:outline-none focus:ring-2 ${
    error
      ? 'border-red-500 focus:ring-red-500 focus:border-red-500'
      : theme === 'light'
        ? 'border-gray-300 focus:ring-blue-500 focus:border-blue-500'
        : 'border-slate-600 focus:ring-blue-500 focus:border-blue-500'
  }`;

  const themeStyles = theme === 'light'
    ? 'bg-white text-gray-900'
    : 'bg-slate-700 text-slate-100';

  // If not searchable, render native select
  if (!isSearchable) {
    return (
      <div className="w-full">
        {label && (
          <label
            htmlFor={inputId}
            className={`block text-sm font-medium mb-1.5 ${
              theme === 'light' ? 'text-gray-700' : 'text-slate-300'
            }`}
          >
            {label}
          </label>
        )}
        <div className="relative">
          <select
            ref={ref}
            id={inputId}
            value={value}
            onChange={onChange}
            className={`${baseStyles} ${themeStyles} ${sizeStyles[inputSize]} appearance-none pr-10 ${className}`}
            {...props}
          >
            {placeholder && (
              <option value="" disabled>
                {placeholder}
              </option>
            )}
            {options.map(option => (
              <option
                key={option.value}
                value={option.value}
                disabled={option.disabled}
              >
                {option.label}
              </option>
            ))}
          </select>
          <div className={`absolute right-3 top-1/2 -translate-y-1/2 pointer-events-none ${
            theme === 'light' ? 'text-gray-400' : 'text-slate-400'
          }`}>
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
            </svg>
          </div>
        </div>
        {error && <p className="mt-1.5 text-sm text-red-500">{error}</p>}
        {helperText && !error && (
          <p className={`mt-1.5 text-sm ${
            theme === 'light' ? 'text-gray-500' : 'text-slate-400'
          }`}>
            {helperText}
          </p>
        )}
      </div>
    );
  }

  // Render custom searchable select
  return (
    <div className="w-full">
      {label && (
        <label
          htmlFor={inputId}
          className={`block text-sm font-medium mb-1.5 ${
            theme === 'light' ? 'text-gray-700' : 'text-slate-300'
          }`}
        >
          {label}
        </label>
      )}
      <div ref={containerRef} className="relative">
        <div
          onClick={() => setIsOpen(!isOpen)}
          onKeyDown={handleKeyDown}
          tabIndex={0}
          role="combobox"
          aria-haspopup="listbox"
          aria-expanded={isOpen}
          className={`${baseStyles} ${themeStyles} ${sizeStyles[inputSize]} cursor-pointer flex items-center justify-between ${
            error ? 'border-red-500' : ''
          } ${className}`}
        >
          <span className={selectedOption ? '' : theme === 'light' ? 'text-gray-400' : 'text-slate-400'}>
            {selectedOption?.label || placeholder}
          </span>
          <svg
            className={`w-5 h-5 transition-transform ${isOpen ? 'rotate-180' : ''} ${
              theme === 'light' ? 'text-gray-400' : 'text-slate-400'
            }`}
            fill="none"
            viewBox="0 0 24 24"
            stroke="currentColor"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
          </svg>
        </div>

        {isOpen && (
          <div
            className={`absolute z-10 w-full mt-1 rounded-lg shadow-lg max-h-60 overflow-auto ${
              theme === 'light' ? 'bg-white border border-gray-200' : 'bg-slate-700 border border-slate-600'
            }`}
          >
            <div className={`p-2 ${
              theme === 'light' ? 'border-b border-gray-200' : 'border-b border-slate-600'
            }`}>
              <input
                type="text"
                value={searchValue}
                onChange={(e) => {
                  setSearchValue(e.target.value);
                  setHighlightedIndex(0);
                }}
                placeholder="Search..."
                className={`w-full rounded-md px-2 py-1 text-sm focus:outline-none focus:ring-1 ${
                  theme === 'light'
                    ? 'bg-gray-100 border border-gray-300 text-gray-900 placeholder-gray-400'
                    : 'bg-slate-600 border border-slate-500 text-slate-100 placeholder-slate-400'
                }`}
                autoFocus
              />
            </div>
            <ul
              ref={listRef}
              role="listbox"
              className="py-1"
            >
              {filteredOptions.length === 0 ? (
                <li className={`px-3 py-2 text-sm ${
                  theme === 'light' ? 'text-gray-500' : 'text-slate-400'
                }`}>
                  No options found
                </li>
              ) : (
                filteredOptions.map((option, index) => (
                  <li
                    key={option.value}
                    role="option"
                    aria-selected={option.value === value}
                    onClick={() => {
                      if (!option.disabled) {
                        const event = {
                          target: { value: option.value }
                        } as React.ChangeEvent<HTMLSelectElement>;
                        onChange?.(event);
                        setIsOpen(false);
                        setSearchValue('');
                      }
                    }}
                    onMouseEnter={() => setHighlightedIndex(index)}
                    className={`px-3 py-2 cursor-pointer transition-colors ${
                      index === highlightedIndex
                        ? theme === 'light'
                          ? 'bg-blue-100 text-blue-900'
                          : 'bg-blue-600 text-white'
                        : option.value === value
                          ? theme === 'light'
                            ? 'bg-blue-50 text-blue-800'
                            : 'bg-blue-900/50 text-blue-200'
                          : theme === 'light'
                            ? 'text-gray-900 hover:bg-gray-100'
                            : 'text-slate-100 hover:bg-slate-600'
                    } ${option.disabled ? 'opacity-50 cursor-not-allowed' : ''}`}
                  >
                    {option.label}
                  </li>
                ))
              )}
            </ul>
          </div>
        )}
      </div>
      {error && <p className="mt-1.5 text-sm text-red-500">{error}</p>}
      {helperText && !error && (
        <p className={`mt-1.5 text-sm ${
          theme === 'light' ? 'text-gray-500' : 'text-slate-400'
        }`}>
          {helperText}
        </p>
      )}
    </div>
  );
});

Select.displayName = 'Select';
