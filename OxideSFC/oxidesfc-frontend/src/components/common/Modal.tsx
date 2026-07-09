import React, { useEffect, useCallback } from 'react';
import { useSettingsStore } from '../../stores/settingsStore';
import { Button } from './Button';

export interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title?: string;
  children: React.ReactNode;
  footer?: React.ReactNode;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  closeOnOverlayClick?: boolean;
  showCloseButton?: boolean;
}

export function Modal({
  isOpen,
  onClose,
  title,
  children,
  footer,
  size = 'md',
  closeOnOverlayClick = true,
  showCloseButton = true,
}: ModalProps) {
  const { settings } = useSettingsStore();
  const theme = settings.general.theme;

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      onClose();
    }
  }, [onClose]);

  useEffect(() => {
    if (isOpen) {
      document.addEventListener('keydown', handleKeyDown);
      document.body.style.overflow = 'hidden';
    }
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      document.body.style.overflow = 'unset';
    };
  }, [isOpen, handleKeyDown]);

  if (!isOpen) return null;

  const sizeStyles = {
    sm: 'max-w-sm',
    md: 'max-w-md',
    lg: 'max-w-lg',
    xl: 'max-w-xl',
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      {/* Overlay */}
      <div
        className="absolute inset-0 bg-black/50 backdrop-blur-sm"
        onClick={closeOnOverlayClick ? onClose : undefined}
        aria-hidden="true"
      />

      {/* Modal Content */}
      <div
        className={`relative w-full ${sizeStyles[size]} rounded-lg shadow-xl animate-slide-in ${
          theme === 'light' ? 'bg-white' : 'bg-slate-800'
        }`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? 'modal-title' : undefined}
      >
        {/* Header */}
        {(title || showCloseButton) && (
          <div className={`flex items-center justify-between p-4 border-b ${
            theme === 'light' ? 'border-gray-200' : 'border-slate-700'
          }`}>
            {title && (
              <h2
                id="modal-title"
                className={`text-lg font-semibold ${
                  theme === 'light' ? 'text-gray-900' : 'text-slate-100'
                }`}
              >
                {title}
              </h2>
            )}
            {showCloseButton && (
              <button
                onClick={onClose}
                className={`p-1 rounded-md transition-colors ${
                  theme === 'light'
                    ? 'text-gray-400 hover:text-gray-600 hover:bg-gray-100'
                    : 'text-slate-400 hover:text-slate-200 hover:bg-slate-700'
                }`}
                aria-label="Close modal"
              >
                <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            )}
          </div>
        )}

        {/* Body */}
        <div className={`p-4 ${
          theme === 'light' ? 'text-gray-700' : 'text-slate-300'
        }`}>
          {children}
        </div>

        {/* Footer */}
        {footer && (
          <div className={`flex items-center justify-end gap-2 p-4 border-t ${
            theme === 'light' ? 'border-gray-200' : 'border-slate-700'
          }`}>
            {footer}
          </div>
        )}
      </div>
    </div>
  );
}

export interface ConfirmModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => void;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  variant?: 'danger' | 'primary';
  isLoading?: boolean;
}

export function ConfirmModal({
  isOpen,
  onClose,
  onConfirm,
  title,
  message,
  confirmText = 'Confirm',
  cancelText = 'Cancel',
  variant = 'primary',
  isLoading = false,
}: ConfirmModalProps) {
  return (
    <Modal
      isOpen={isOpen}
      onClose={onClose}
      title={title}
      footer={
        <>
          <Button variant="ghost" onClick={onClose} disabled={isLoading}>
            {cancelText}
          </Button>
          <Button variant={variant} onClick={onConfirm} isLoading={isLoading}>
            {confirmText}
          </Button>
        </>
      }
    >
      <p>{message}</p>
    </Modal>
  );
}
