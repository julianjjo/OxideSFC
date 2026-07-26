import React, { forwardRef } from 'react';
import { useControlId } from './useControlId';

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  label?: string;
  error?: string;
  helperText?: string;
  leftIcon?: React.ReactNode;
  rightIcon?: React.ReactNode;
  inputSize?: 'sm' | 'md' | 'lg';
}

const SIZES = {
  sm: 'h-8 px-2.5 text-[0.8125rem]',
  md: 'h-9 px-3 text-sm',
  lg: 'h-11 px-4 text-base',
} as const;

const ICON_PAD = {
  sm: { left: 'pl-8', right: 'pr-8' },
  md: { left: 'pl-9', right: 'pr-9' },
  lg: { left: 'pl-11', right: 'pr-11' },
} as const;

export const Input = forwardRef<HTMLInputElement, InputProps>(
  (
    {
      label,
      error,
      helperText,
      leftIcon,
      rightIcon,
      inputSize = 'md',
      className = '',
      id,
      ...props
    },
    ref
  ) => {
    const inputId = useControlId(id, 'input');
    const describedBy = error
      ? `${inputId}-error`
      : helperText
        ? `${inputId}-help`
        : undefined;

    return (
      <div className="w-full">
        {label && (
          <label htmlFor={inputId} className="field-row-label mb-1.5 block">
            {label}
          </label>
        )}
        <div className="relative">
          {leftIcon && (
            <span className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-mute">
              {leftIcon}
            </span>
          )}
          <input
            ref={ref}
            id={inputId}
            aria-invalid={error ? true : undefined}
            aria-describedby={describedBy}
            className={`field ${SIZES[inputSize]} ${error ? 'field--invalid' : ''} ${
              leftIcon ? ICON_PAD[inputSize].left : ''
            } ${rightIcon ? ICON_PAD[inputSize].right : ''} ${className}`}
            {...props}
          />
          {rightIcon && (
            <span className="pointer-events-none absolute right-2.5 top-1/2 -translate-y-1/2 text-mute">
              {rightIcon}
            </span>
          )}
        </div>
        {error && (
          <p id={`${inputId}-error`} className="mt-1.5 text-[0.8125rem] text-danger-text">
            {error}
          </p>
        )}
        {helperText && !error && (
          <p id={`${inputId}-help`} className="field-row-help">
            {helperText}
          </p>
        )}
      </div>
    );
  }
);

Input.displayName = 'Input';

export interface TextAreaProps
  extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  label?: string;
  error?: string;
  helperText?: string;
}

export const TextArea = forwardRef<HTMLTextAreaElement, TextAreaProps>(
  ({ label, error, helperText, className = '', id, ...props }, ref) => {
    const inputId = useControlId(id, 'textarea');

    return (
      <div className="w-full">
        {label && (
          <label htmlFor={inputId} className="field-row-label mb-1.5 block">
            {label}
          </label>
        )}
        <textarea
          ref={ref}
          id={inputId}
          aria-invalid={error ? true : undefined}
          className={`field px-3 py-2 text-sm ${error ? 'field--invalid' : ''} ${className}`}
          {...props}
        />
        {error && <p className="mt-1.5 text-[0.8125rem] text-danger-text">{error}</p>}
        {helperText && !error && <p className="field-row-help">{helperText}</p>}
      </div>
    );
  }
);

TextArea.displayName = 'TextArea';
