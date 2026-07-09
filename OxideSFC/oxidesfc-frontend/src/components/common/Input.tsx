import React, { forwardRef } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
  helperText?: string;
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
  inputSize?: 'sm' | 'md' | 'lg';
}

export const Input = forwardRef<HTMLInputElement, InputProps>(({
  label,
  error,
  helperText,
  leftIcon,
  rightIcon,
  inputSize = 'md',
  className = '',
  id,
  ...props
}, ref) => {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme;

  const inputId = id || `input-${Math.random().toString(36).substr(2, 9)}`;

  const sizeStyles = {
    sm: 'px-2.5 py-1.5 text-sm',
    md: 'px-3 py-2 text-base',
    lg: 'px-4 py-3 text-lg',
  };

  const iconSizeStyles = {
    sm: 'w-4 h-4',
    md: 'w-5 h-5',
    lg: 'w-6 h-6',
  };

  const baseInputStyles = `w-full rounded-lg transition-colors focus:outline-none focus:ring-2 ${
    error
      ? theme === 'light'
        ? 'border-red-500 focus:ring-red-500 focus:border-red-500'
        : 'border-red-500 focus:ring-red-500 focus:border-red-500'
      : theme === 'light'
        ? 'border-gray-300 focus:ring-blue-500 focus:border-blue-500'
        : 'border-slate-600 focus:ring-blue-500 focus:border-blue-500'
  }`;

  const themeInputStyles = theme === 'light'
    ? 'bg-white text-gray-900 placeholder-gray-400'
    : 'bg-slate-700 text-slate-100 placeholder-slate-400';

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
        {leftIcon && (
          <div className={`absolute left-3 top-1/2 -translate-y-1/2 ${iconSizeStyles[inputSize]} ${
            theme === 'light' ? 'text-gray-400' : 'text-slate-400'
          }`}>
            {leftIcon}
          </div>
        )}
        <input
          ref={ref}
          id={inputId}
          className={`${baseInputStyles} ${themeInputStyles} ${sizeStyles[inputSize]} ${
            leftIcon ? 'pl-10' : ''
          } ${rightIcon ? 'pr-10' : ''} ${error ? 'border-red-500' : ''} ${className}`}
          {...props}
        />
        {rightIcon && (
          <div className={`absolute right-3 top-1/2 -translate-y-1/2 ${iconSizeStyles[inputSize]} ${
            theme === 'light' ? 'text-gray-400' : 'text-slate-400'
          }`}>
            {rightIcon}
          </div>
        )}
      </div>
      {error && (
        <p className="mt-1.5 text-sm text-red-500">{error}</p>
      )}
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

Input.displayName = 'Input';

export interface TextAreaProps extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  label?: string;
  error?: string;
  helperText?: string;
  inputSize?: 'sm' | 'md' | 'lg';
}

export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(({
  label,
  error,
  helperText,
  inputSize = 'md',
  className = '',
  id,
  ...props
}, ref) => {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme;

  const inputId = id || `textarea-${Math.random().toString(36).substr(2, 9)}`;

  const sizeStyles = {
    sm: 'px-2.5 py-1.5 text-sm',
    md: 'px-3 py-2 text-base',
    lg: 'px-4 py-3 text-lg',
  };

  const baseStyles = `w-full rounded-lg transition-colors focus:outline-none focus:ring-2 ${
    error
      ? 'border-red-500 focus:ring-red-500 focus:border-red-500'
      : theme === 'light'
        ? 'border-gray-300 focus:ring-blue-500 focus:border-blue-500'
        : 'border-slate-600 focus:ring-blue-500 focus:border-blue-500'
  }`;

  const themeStyles = theme === 'light'
    ? 'bg-white text-gray-900 placeholder-gray-400'
    : 'bg-slate-700 text-slate-100 placeholder-slate-400';

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
      <textarea
        ref={ref}
        id={inputId}
        className={`${baseStyles} ${themeStyles} ${sizeStyles[inputSize]} ${className}`}
        {...props}
      />
      {error && (
        <p className="mt-1.5 text-sm text-red-500">{error}</p>
      )}
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

TextArea.displayName = 'TextArea';
