import React from 'react';

export type ButtonVariant = 'primary' | 'secondary' | 'danger' | 'ghost';
export type ButtonSize = 'sm' | 'md' | 'lg';

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  isLoading?: boolean;
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
  /** Stretch to the width of the container. */
  block?: boolean;
}

const SIZES: Record<ButtonSize, string> = {
  sm: 'h-8 px-3 text-[0.8125rem]',
  md: 'h-9 px-4 text-sm',
  lg: 'h-11 px-5 text-base',
};

/**
 * Colour and hover behaviour live in `.btn--*` (src/styles/index.css) so the
 * button follows the active theme and accent without this component knowing
 * which either of those is.
 */
export function Button({
  children,
  variant = 'primary',
  size = 'md',
  isLoading = false,
  leftIcon,
  rightIcon,
  block = false,
  disabled,
  className = '',
  ...props
}: ButtonProps) {
  return (
    <button
      className={`btn btn--${variant} ${SIZES[size]} ${block ? 'w-full' : ''} ${className}`}
      disabled={disabled || isLoading}
      {...props}
    >
      {isLoading ? (
        <svg className="animate-spin h-4 w-4" fill="none" viewBox="0 0 24 24" aria-hidden>
          <circle
            className="opacity-25"
            cx="12"
            cy="12"
            r="10"
            stroke="currentColor"
            strokeWidth="4"
          />
          <path
            className="opacity-75"
            fill="currentColor"
            d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z"
          />
        </svg>
      ) : (
        leftIcon
      )}
      {children}
      {rightIcon && !isLoading && rightIcon}
    </button>
  );
}

/** Square icon-only button. Needs an accessible label from the caller. */
export interface IconButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  'aria-label': string;
}

const ICON_SIZES: Record<ButtonSize, string> = {
  sm: 'h-8 w-8',
  md: 'h-9 w-9',
  lg: 'h-11 w-11',
};

export function IconButton({
  children,
  variant = 'ghost',
  size = 'md',
  className = '',
  ...props
}: IconButtonProps) {
  return (
    <button
      className={`btn btn--${variant} ${ICON_SIZES[size]} p-0 ${className}`}
      {...props}
    >
      {children}
    </button>
  );
}
