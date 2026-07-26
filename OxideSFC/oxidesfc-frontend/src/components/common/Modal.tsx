import React, { useEffect, useCallback, useId, useRef } from 'react';
import { Button } from './Button';

/**
 * Open modals, innermost last.
 *
 * Escape must only reach the topmost dialog. Every open `Modal` attaches its own
 * `document` keydown listener, so without a stack a nested confirmation and its
 * parent both fired: backing out of "Remove from library" with Escape also closed
 * the game details panel behind it and dumped the user back to the shelf.
 * `stopPropagation` cannot fix that — both listeners are on `document`, so
 * neither is in the other's propagation path.
 */
const openModals: symbol[] = [];

export interface ModalProps {
  isOpen: boolean;
  onClose: () => void;
  title?: string;
  /** Optional line under the title: what this dialog acts on. */
  subtitle?: string;
  children: React.ReactNode;
  footer?: React.ReactNode;
  size?: 'sm' | 'md' | 'lg' | 'xl';
  closeOnOverlayClick?: boolean;
  showCloseButton?: boolean;
}

const SIZES = {
  sm: 'max-w-sm',
  md: 'max-w-md',
  lg: 'max-w-2xl',
  xl: 'max-w-4xl',
} as const;

export function Modal({
  isOpen,
  onClose,
  title,
  subtitle,
  children,
  footer,
  size = 'md',
  closeOnOverlayClick = true,
  showCloseButton = true,
}: ModalProps) {
  const panelRef = useRef<HTMLDivElement>(null);
  // Identity for the open-modal stack. A ref so it survives re-renders and stays
  // unique per instance.
  const stackTokenRef = useRef<symbol>(Symbol('modal'));
  // Unique per instance, so two dialogs open at once do not both claim
  // `id="modal-title"` -- which had them share one id and made every
  // `aria-labelledby` resolve to the first, announcing the wrong dialog's name.
  const titleId = `modal-title-${useId()}`;

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      if (e.key !== 'Escape') return;
      // Only the innermost dialog reacts.
      if (openModals[openModals.length - 1] !== stackTokenRef.current) return;
      onClose();
    },
    [onClose]
  );

  useEffect(() => {
    if (!isOpen) return;
    const token = stackTokenRef.current;
    openModals.push(token);
    document.addEventListener('keydown', handleKeyDown);
    return () => {
      document.removeEventListener('keydown', handleKeyDown);
      const index = openModals.lastIndexOf(token);
      if (index !== -1) openModals.splice(index, 1);
    };
  }, [isOpen, handleKeyDown]);

  // Move focus into the dialog on open so keyboard users land inside it rather
  // than continuing to tab through the page behind the scrim.
  useEffect(() => {
    if (!isOpen) return;
    const focusable = panelRef.current?.querySelector<HTMLElement>(
      'input, select, textarea, button, [href], [tabindex]:not([tabindex="-1"])'
    );
    focusable?.focus();
  }, [isOpen]);

  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      <div
        className="absolute inset-0 animate-fade-in"
        style={{ background: 'var(--scrim)', backdropFilter: 'blur(3px)' }}
        onClick={closeOnOverlayClick ? onClose : undefined}
        aria-hidden="true"
      />

      <div
        ref={panelRef}
        className={`panel pinstripe-top animate-slide-in relative flex max-h-[calc(100vh-2rem)] w-full flex-col overflow-hidden shadow-lg ${SIZES[size]}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? titleId : undefined}
      >
        {(title || showCloseButton) && (
          <div className="flex items-start justify-between gap-4 border-b border-line px-5 pb-4 pt-5">
            <div className="min-w-0">
              {title && (
                <h2 id={titleId} className="display-md truncate text-ink">
                  {title}
                </h2>
              )}
              {subtitle && <p className="register mt-1 truncate">{subtitle}</p>}
            </div>
            {showCloseButton && (
              <button
                onClick={onClose}
                className="btn btn--ghost -mr-1 -mt-1 h-8 w-8 flex-none p-0"
                aria-label="Close"
              >
                <svg
                  className="h-4 w-4"
                  fill="none"
                  viewBox="0 0 24 24"
                  stroke="currentColor"
                  strokeWidth={2}
                  aria-hidden
                >
                  <path strokeLinecap="round" strokeLinejoin="round" d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
            )}
          </div>
        )}

        <div className="min-h-0 flex-1 overflow-auto px-5 py-4 text-sm text-dim">
          {children}
        </div>

        {footer && (
          <div className="flex items-center justify-end gap-2 border-t border-line px-5 py-4">
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
      size="sm"
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
      <p className="leading-relaxed">{message}</p>
    </Modal>
  );
}
