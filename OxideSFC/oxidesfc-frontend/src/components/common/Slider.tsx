import React, { forwardRef, useState, useCallback } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';

export interface SliderProps extends Omit<React.InputHTMLAttributes<HTMLInputElement>, 'type' | 'size'> {
  label?: string;
  helperText?: string;
  showValue?: boolean;
  valueDisplay?: (value: number) => string;
  size?: 'sm' | 'md' | 'lg';
  showMinMax?: boolean;
}

export const Slider = forwardRef<HTMLInputElement, SliderProps>(({
  label,
  helperText,
  showValue = true,
  valueDisplay,
  size = 'md',
  showMinMax = false,
  className = '',
  id,
  value,
  min,
  max,
  step,
  onChange,
  ...props
}, ref) => {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme;

  const [isDragging, setIsDragging] = useState(false);

  const inputId = id || `slider-${Math.random().toString(36).substr(2, 9)}`;

  const sizeStyles = {
    sm: {
      track: 'h-1',
      thumb: 'w-4 h-4',
      thumbActive: 'w-5 h-5',
    },
    md: {
      track: 'h-2',
      thumb: 'w-5 h-5',
      thumbActive: 'w-6 h-6',
    },
    lg: {
      track: 'h-3',
      thumb: 'w-6 h-6',
      thumbActive: 'w-7 h-7',
    },
  };

  const percentage = min !== undefined && max !== undefined
    ? ((Number(value) - Number(min)) / (Number(max) - Number(min))) * 100
    : 0;

  const handleChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    onChange?.(e);
  }, [onChange]);

  const displayValue = valueDisplay
    ? valueDisplay(Number(value))
    : String(value);

  return (
    <div className={`w-full ${className}`}>
      {(label || showValue) && (
        <div className="flex items-center justify-between mb-2">
          {label && (
            <label
              htmlFor={inputId}
              className={`text-sm font-medium ${
                theme === 'light' ? 'text-gray-700' : 'text-slate-300'
              }`}
            >
              {label}
            </label>
          )}
          {showValue && (
            <span className={`text-sm ${
              theme === 'light' ? 'text-gray-600' : 'text-slate-400'
            }`}>
              {displayValue}
            </span>
          )}
        </div>
      )}
      
      <div className="relative">
        {/* Track background */}
        <div
          className={`absolute inset-0 rounded-full ${
            theme === 'light' ? 'bg-gray-200' : 'bg-slate-600'
          } ${sizeStyles[size].track}`}
        />
        
        {/* Track fill */}
        <div
          className={`absolute rounded-full bg-blue-600 transition-all ${
            sizeStyles[size].track
          }`}
          style={{ width: `${percentage}%` }}
        />

        {/* Input */}
        <input
          ref={ref}
          type="range"
          id={inputId}
          value={value}
          min={min}
          max={max}
          step={step}
          onChange={handleChange}
          onMouseDown={() => setIsDragging(true)}
          onMouseUp={() => setIsDragging(false)}
          onTouchStart={() => setIsDragging(true)}
          onTouchEnd={() => setIsDragging(false)}
          className={`
            relative w-full h-0 appearance-none cursor-pointer
            bg-transparent
            focus:outline-none focus:ring-2 focus:ring-blue-500 focus:ring-offset-2
            ${theme === 'light' ? 'focus:ring-offset-white' : 'focus:ring-offset-slate-800'}
            
            /* Thumb styles */
            [&::-webkit-slider-thumb]:appearance-none
            [&::-webkit-slider-thumb]:rounded-full
            [&::-webkit-slider-thumb]:bg-white
            [&::-webkit-slider-thumb]:shadow-md
            [&::-webkit-slider-thumb]:cursor-pointer
            [&::-webkit-slider-thumb]:transition-transform
            [&::-webkit-slider-thumb]:duration-150
            [&::-webkit-slider-thumb]:ease-in-out
            [&::-webkit-slider-thumb]:${sizeStyles[size].thumb}
            [&::-webkit-slider-thumb]:${isDragging ? sizeStyles[size].thumbActive : ''}
            [&::-webkit-slider-thumb]:hover:scale-110
            [&::-webkit-slider-thumb]:active:scale-95
            
            /* Firefox thumb styles */
            [&::-moz-range-thumb]:rounded-full
            [&::-moz-range-thumb]:bg-white
            [&::-moz-range-thumb]:border-0
            [&::-moz-range-thumb]:shadow-md
            [&::-moz-range-thumb]:cursor-pointer
            [&::-moz-range-thumb]:transition-transform
            [&::-moz-range-thumb]:duration-150
            [&::-moz-range-thumb]:ease-in-out
            [&::-moz-range-thumb]:${sizeStyles[size].thumb}
            [&::-moz-range-thumb]:${isDragging ? sizeStyles[size].thumbActive : ''}
            [&::-moz-range-thumb]:hover:scale-110
            [&::-moz-range-thumb]:active:scale-95
            [&::-moz-range-thumb]:appearance-none
            
            /* Track styles for webkit */
            [&::-webkit-slider-runnable-track]:rounded-full
            [&::-webkit-slider-runnable-track]:h-full
            [&::-webkit-slider-runnable-track]:appearance-none
            [&::-webkit-slider-runnable-track]:bg-transparent
            
            /* Track styles for Firefox */
            [&::-moz-range-track]:rounded-full
            [&::-moz-range-track]:h-full
            [&::-moz-range-track]:bg-transparent
          `}
          {...props}
        />
      </div>

      {showMinMax && (
        <div className={`flex justify-between mt-1 text-xs ${
          theme === 'light' ? 'text-gray-500' : 'text-slate-400'
        }`}>
          <span>{valueDisplay ? valueDisplay(Number(min!)) : min}</span>
          <span>{valueDisplay ? valueDisplay(Number(max!)) : max}</span>
        </div>
      )}

      {helperText && (
        <p className={`mt-1.5 text-sm ${
          theme === 'light' ? 'text-gray-500' : 'text-slate-400'
        }`}>
          {helperText}
        </p>
      )}
    </div>
  );
});

Slider.displayName = 'Slider';

// Volume-specific slider with icon
export interface VolumeSliderProps extends Omit<SliderProps, 'valueDisplay'> {
  showVolumeIcon?: boolean;
}

export const VolumeSlider = forwardRef<HTMLInputElement, VolumeSliderProps>(({
  showVolumeIcon = true,
  ...props
}, ref) => {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme;

  const value = Number(props.value);
  const max = Number(props.max) || 100;

  const getVolumeIcon = () => {
    if (value === 0) {
      return (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5.586 15H4a1 1 0 01-1-1v-4a1 1 0 011-1h1.586l4.707-4.707C10.923 3.663 12 4.109 12 5v14c0 .891-1.077 1.337-1.707.707L5.586 15z" />
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M17 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2" />
        </svg>
      );
    }
    if (value < max / 3) {
      return (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5.586 15H4a1 1 0 01-1-1v-4a1 1 0 011-1h1.586l4.707-4.707C10.923 3.663 12 4.109 12 5v14c0 .891-1.077 1.337-1.707.707L5.586 15z" />
        </svg>
      );
    }
    if (value < max * 2 / 3) {
      return (
        <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
          <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15.536 8.464a5 5 0 010 7.072M5.586 15H4a1 1 0 01-1-1v-4a1 1 0 011-1h1.586l4.707-4.707C10.923 3.663 12 4.109 12 5v14c0 .891-1.077 1.337-1.707.707L5.586 15z" />
        </svg>
      );
    }
    return (
      <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15.536 8.464a5 5 0 010 7.072m2.828-9.9a9 9 0 010 12.728M5.586 15H4a1 1 0 01-1-1v-4a1 1 0 011-1h1.586l4.707-4.707C10.923 3.663 12 4.109 12 5v14c0 .891-1.077 1.337-1.707.707L5.586 15z" />
      </svg>
    );
  };

  return (
    <div className="flex items-center gap-3">
      {showVolumeIcon && (
        <span className={theme === 'light' ? 'text-gray-500' : 'text-slate-400'}>
          {getVolumeIcon()}
        </span>
      )}
      <Slider
        ref={ref}
        {...props}
        valueDisplay={(v) => `${Math.round((v / max) * 100)}%`}
      />
    </div>
  );
});

VolumeSlider.displayName = 'VolumeSlider';
