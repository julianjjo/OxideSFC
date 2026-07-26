import React, { forwardRef } from 'react';
import { useControlId } from './useControlId';

export interface SliderProps
  extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'> {
  label?: string;
  helperText?: string;
  showValue?: boolean;
  valueDisplay?: (value: number) => string;
  showMinMax?: boolean;
}

/**
 * Range input.
 *
 * The filled portion of the track is painted by the `.range` rules in
 * index.css from a `--pct` custom property set here. The previous version
 * stacked three absolutely-positioned divs behind a zero-height input to fake
 * the fill, which drifted out of alignment with the thumb at both extremes
 * (the thumb has width, so its centre never reaches 0% or 100% of the track).
 */
export const Slider = forwardRef<HTMLInputElement, SliderProps>(
  (
    {
      label,
      helperText,
      showValue = true,
      valueDisplay,
      showMinMax = false,
      className = '',
      id,
      value,
      min = 0,
      max = 100,
      step,
      disabled,
      ...props
    },
    ref
  ) => {
    const inputId = useControlId(id, 'slider');

    const numericValue = Number(value);
    const numericMin = Number(min);
    const numericMax = Number(max);
    const span = numericMax - numericMin;
    // Guard the degenerate min === max case, which would otherwise put NaN into
    // the gradient and drop the fill entirely.
    const pct = span > 0 ? ((numericValue - numericMin) / span) * 100 : 0;

    const display = valueDisplay ? valueDisplay(numericValue) : String(value);
    const format = (v: number) => (valueDisplay ? valueDisplay(v) : String(v));

    return (
      <div className={`w-full ${disabled ? 'opacity-60' : ''} ${className}`}>
        {(label || showValue) && (
          <div className="mb-1.5 flex items-baseline justify-between gap-3">
            {label && (
              <label htmlFor={inputId} className="field-row-label">
                {label}
              </label>
            )}
            {showValue && <span className="register text-ink">{display}</span>}
          </div>
        )}

        <input
          ref={ref}
          type="range"
          id={inputId}
          className="range"
          style={{ ['--pct' as string]: `${Math.min(100, Math.max(0, pct))}%` }}
          value={value}
          min={min}
          max={max}
          step={step}
          disabled={disabled}
          {...props}
        />

        {showMinMax && (
          <div className="mt-0.5 flex justify-between">
            <span className="register">{format(numericMin)}</span>
            <span className="register">{format(numericMax)}</span>
          </div>
        )}

        {helperText && <p className="field-row-help">{helperText}</p>}
      </div>
    );
  }
);

Slider.displayName = 'Slider';
