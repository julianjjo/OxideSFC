import React, { forwardRef, useId } from 'react';

export interface ToggleProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'> {
  label?: string;
  description?: string;
  /** `sm` for dense lists (one row per item); `md` everywhere else. */
  size?: 'sm' | 'md';
}

/**
 * A switch, rendered as a real focusable `<button role="switch">` with a hidden
 * checkbox kept in step for form semantics.
 *
 * The synthetic change event below exists because every call site in the app
 * reads `e.target.checked`; the button has no `checked` of its own to report, so
 * the handler is invoked with a minimal object shaped like the input event those
 * callers already expect.
 */
export const Toggle = forwardRef<HTMLInputElement, ToggleProps>(
  (
    {
      label,
      description,
      size = 'md',
      className = '',
      id,
      checked,
      disabled,
      onChange,
      // The switch the user actually operates is the button, not the hidden
      // checkbox, so an explicit `aria-label` has to be routed there. Spreading
      // it onto the input along with the rest of `props` left the button with no
      // accessible name at all -- which is every settings row, since those put
      // the visible label in the row rather than on the control.
      'aria-label': ariaLabel,
      ...props
    },
    ref
  ) => {
    // useId is stable across renders, unlike the Math.random() this used to
    // generate -- a fresh id on every render broke the label's htmlFor link.
    const generatedId = useId();
    const inputId = id || `toggle-${generatedId}`;

    const emitChange = () => {
      if (disabled) return;
      onChange?.({
        target: { checked: !checked },
      } as React.ChangeEvent<HTMLInputElement>);
    };

    return (
      <div className={`flex items-start gap-3 ${className}`}>
        <input
          ref={ref}
          type="checkbox"
          id={inputId}
          checked={checked}
          disabled={disabled}
          onChange={onChange}
          className="sr-only"
          tabIndex={-1}
          {...props}
        />
        <button
          type="button"
          role="switch"
          aria-checked={!!checked}
          aria-label={ariaLabel}
          aria-labelledby={!ariaLabel && label ? `${inputId}-label` : undefined}
          disabled={disabled}
          onClick={emitChange}
          className={`switch ${size === 'sm' ? 'switch--sm' : ''} ${
            checked ? 'switch--on' : ''
          } mt-0.5`}
        />
        {(label || description) && (
          <div className="min-w-0">
            {label && (
              <label
                id={`${inputId}-label`}
                htmlFor={inputId}
                className="field-row-label block cursor-pointer"
              >
                {label}
              </label>
            )}
            {description && <p className="field-row-help">{description}</p>}
          </div>
        )}
      </div>
    );
  }
);

Toggle.displayName = 'Toggle';
