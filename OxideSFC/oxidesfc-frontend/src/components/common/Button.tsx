import React from 'react';
import { useSettingsStore } from '../../stores/settingsStore';

export type ButtonVariant = 'primary' | 'secondary' | 'danger' | 'ghost';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  isLoading?: boolean;
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
}

export function Button({
  children,
  variant = 'primary',
  size = 'md',
  isLoading = false,
  leftIcon,
  rightIcon,
  disabled,
  className = '',
  ...props
}: ButtonProps) {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme;

  const baseStyles = 'inline-flex items-center justify-center font-medium rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-offset-2 disabled:opacity-50 disabled:cursor-not-allowed';

  const variantStyles: Record<ButtonVariant, string> = {
    primary: theme === 'light'
      ? 'bg-blue-600 text-white hover:bg-blue-700 focus:ring-blue-500'
      : 'bg-blue-600 text-white hover:bg-blue-700 focus:ring-blue-500',
    secondary: theme === 'light'
      ? 'bg-gray-200 text-gray-800 hover:bg-gray-300 focus:ring-gray-500 border border-gray-300'
      : 'bg-slate-700 text-slate-200 hover:bg-slate-600 focus:ring-slate-500 border border-slate-600',
    danger: theme === 'light'
      ? 'bg-red-600 text-white hover:bg-red-700 focus:ring-red-500'
      : 'bg-red-600 text-white hover:bg-red-700 focus:ring-red-500',
    ghost: theme === 'light'
      ? 'bg-transparent text-gray-700 hover:bg-gray-100 focus:ring-gray-500'
      : 'bg-transparent text-slate-300 hover:bg-slate-800 focus:ring-slate-500',
  };

  const sizeStyles: Record<ButtonSize, string> = {
    sm: 'px-3 py-1.5 text-sm',
    md: 'px-4 py-2 text-base',
    lg: 'px-6 py-3 text-lg',
  };

  return (
    <button
      className={`${baseStyles} ${variantStyles[variant]} ${sizeStyles[size]} ${className}`}
      disabled={disabled || isLoading}
      {...props}
    >
      {isLoading ? (
        <svg className="animate-spin -ml-1 mr-2 h-4 w-4" fill="none" viewBox="0 0 24 24">
          <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
          <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
        </svg>
      ) : leftIcon ? (
        <span className="mr-2">{leftIcon}</span>
      ) : null}
      {children}
      {rightIcon && !isLoading && <span className="ml-2">{rightIcon}</span>}
    </button>
  );
}
