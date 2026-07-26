import React, { forwardRef } from 'react';
import { useControlId } from './useControlId';

export interface SelectOption {
  value: string;
  label: string;
  disabled?: boolean;
}

export interface SelectProps
  extends Omit<React.SelectHTMLAttributes<HTMLSelectElement>, 'size'> {
  label?: string;
  error?: string;
  helperText?: string;
  options: SelectOption[];
  placeholder?: string;
  inputSize?: 'sm' | 'md' | 'lg';
}

const SIZES = {
  sm: 'h-8 pl-2.5 pr-8 text-[0.8125rem]',
  md: 'h-9 pl-3 pr-9 text-sm',
  lg: 'h-11 pl-4 pr-10 text-base',
} as const;

function Chevron() {
  return (
    <svg
      className="h-4 w-4"
      fill="none"
      viewBox="0 0 24 24"
      stroke="currentColor"
      strokeWidth={2}
      aria-hidden
    >
      <path strokeLinecap="round" strokeLinejoin="round" d="M6 9l6 6 6-6" />
    </svg>
  );
}

/**
 * A native `<select>` in token-styled chrome.
 *
 * This used to carry a second, hand-rolled combobox implementation behind an
 * `isSearchable` prop -- ~180 lines with its own keyboard handling, filtering
 * and outside-click logic, and not a single call site in the app that enabled
 * it. It has been dropped rather than restyled: the native control gets correct
 * keyboard behaviour, screen-reader support and OS-native popup rendering for
 * free, which matters more here than a search box over lists that top out at
 * six options.
 */
export const Select = forwardRef<HTMLSelectElement, SelectProps>(
  (
    {
      label,
      error,
      helperText,
      options,
      placeholder,
      inputSize = 'md',
      className = '',
      id,
      // Destructured (rather than left in `props`) so the unsupported-value check
      // below can see it; passed back to the element explicitly.
      value,
      ...props
    },
    ref
  ) => {
    const selectId = useControlId(id, 'select');

    return (
      <div className="w-full">
        {label && (
          <label htmlFor={selectId} className="field-row-label mb-1.5 block">
            {label}
          </label>
        )}
        <div className="relative">
          <select
            ref={ref}
            id={selectId}
            value={value}
            aria-invalid={error ? true : undefined}
            className={`field appearance-none ${SIZES[inputSize]} ${
              error ? 'field--invalid' : ''
            } ${className}`}
            {...props}
          >
            {placeholder && (
              <option value="" disabled>
                {placeholder}
              </option>
            )}
            {/*
              Surface a persisted value that no longer has an option.
              Without this the browser displays the *first* option while the
              stored value stays whatever it was, and picking that option fires no
              change event (`select.value` never changes), so the stale value is
              unclearable through the UI. It bites on upgrade: a settings.json with
              `shader: 'xbrz'` hits a list that is now only none|crt, because the
              upscalers moved to the scale-mode control.
            */}
            {value !== undefined &&
              value !== '' &&
              !options.some((option) => option.value === value) && (
                <option value={String(value)}>{String(value)} (unsupported)</option>
              )}
            {options.map((option) => (
              <option key={option.value} value={option.value} disabled={option.disabled}>
                {option.label}
              </option>
            ))}
          </select>
          <span className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-mute">
            <Chevron />
          </span>
        </div>
        {error && <p className="mt-1.5 text-[0.8125rem] text-danger-text">{error}</p>}
        {helperText && !error && <p className="field-row-help">{helperText}</p>}
      </div>
    );
  }
);

Select.displayName = 'Select';
