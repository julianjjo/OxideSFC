import React, { forwardRef } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';

export interface ToggleProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'> {
  label?: string;
  description?: string;
  size?: 'sm' | 'md' | 'lg';
}

export const Toggle = forwardRef<HTMLInputElement, ToggleProps>(({
  label,
  description,
  size = 'md',
  className = '',
  id,
  checked,
  onChange,
  ...props
}, ref) => {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme;

  const inputId = id || `toggle-${Math.random().toString(36).substr(2, 9)}`;

  const sizeStyles = {
    sm: {
      track: 'w-8 h-4',
      thumb: 'w-3 h-3',
      translate: 'translate-x-4',
    },
    md: {
      track: 'w-11 h-6',
      thumb: 'w-5 h-5',
      translate: 'translate-x-5',
    },
    lg: {
      track: 'w-14 h-7',
      thumb: 'w-6 h-6',
      translate: 'translate-x-7',
    },
  };

  return (
    <div className={`flex items-start ${className}`}>
      <div className="relative flex-shrink-0">
        <input
          ref={ref}
          type="checkbox"
          id={inputId}
          checked={checked}
          onChange={onChange}
          className="sr-only"
          {...props}
        />
        <button
          type="button"
          role="switch"
          aria-checked={checked}
          onClick={() => {
            const event = {
              target: { checked: !checked }
            } as React.ChangeEvent<HTMLInputElement>;
            onChange?.(event);
          }}
          className={`
            ${sizeStyles[size].track}
            ${checked
              ? 'bg-blue-600'
              : theme === 'light'
                ? 'bg-gray-300'
                : 'bg-slate-600'
            }
            relative inline-flex flex-shrink-0 cursor-pointer rounded-full transition-colors duration-200 ease-in-out focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2
            ${theme === 'light' ? 'focus:ring-offset-white' : 'focus:ring-offset-slate-800'}
          `}
        >
          <span
            className={`
              ${sizeStyles[size].thumb}
              ${size === 'sm' ? 'left-0.5 top-0.5' : size === 'md' ? 'left-0.5 top-0.5' : 'left-0.5 top-0.5'}
              ${checked ? sizeStyles[size].translate : 'translate-x-0'}
              pointer-events-none inline-block rounded-full bg-white shadow transform ring-0 transition duration-200 ease-in-out
            `}
          />
        </button>
      </div>
      {(label || description) && (
        <div className="ml-3">
          {label && (
            <label
              htmlFor={inputId}
              className={`text-sm font-medium ${
                theme === 'light' ? 'text-gray-900' : 'text-slate-100'
              }`}
            >
              {label}
            </label>
          )}
          {description && (
            <p className={`text-sm ${
              theme === 'light' ? 'text-gray-500' : 'text-slate-400'
            }`}>
              {description}
            </p>
          )}
        </div>
      )}
    </div>
  );
});

Toggle.displayName = 'Toggle';
