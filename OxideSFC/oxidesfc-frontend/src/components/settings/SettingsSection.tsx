import { useId, type ReactNode } from 'react';
import { IconInfo } from '../common/icons';
import { SettingRowIdContext } from '../common/useControlId';

interface SettingsSectionProps {
  /**
   * The hardware or subsystem this section governs, e.g. "PPU / OUTPUT".
   *
   * This is the structural device the settings screens are built on, and it is
   * deliberately not decorative: an emulator's settings are a control panel for
   * specific silicon, so naming the silicon tells you why a group of controls
   * belongs together and where to look for a setting you half-remember.
   */
  eyebrow: string;
  title: string;
  description?: string;
  /** Right-aligned control in the section header (usually a master toggle). */
  action?: ReactNode;
  children: ReactNode;
}

export function SettingsSection({
  eyebrow,
  title,
  description,
  action,
  children,
}: SettingsSectionProps) {
  return (
    <section className="panel px-5 py-4">
      <header className="flex items-start justify-between gap-4 pb-1">
        <div className="min-w-0">
          <p className="eyebrow">{eyebrow}</p>
          <h2 className="display-md mt-1 text-ink">{title}</h2>
          {description && (
            <p className="mt-1 max-w-prose text-[0.8125rem] leading-relaxed text-mute">
              {description}
            </p>
          )}
        </div>
        {action && <div className="flex-none pt-1">{action}</div>}
      </header>
      <div className="mt-2">{children}</div>
    </section>
  );
}

/** A label/description + control row. */
interface SettingRowProps {
  label: string;
  help?: string;
  /** Override the generated id, for a row holding more than one control. */
  htmlFor?: string;
  children: ReactNode;
}

export function SettingRow({ label, help, htmlFor, children }: SettingRowProps) {
  const generatedId = useId();
  const controlId = htmlFor ?? `setting-${generatedId}`;

  return (
    <div className="field-row">
      <div className="min-w-0">
        <label htmlFor={controlId} className="field-row-label">
          {label}
        </label>
        {help && <p className="field-row-help max-w-prose">{help}</p>}
      </div>
      <div className="field-row-control">
        <SettingRowIdContext.Provider value={controlId}>
          {children}
        </SettingRowIdContext.Provider>
      </div>
    </div>
  );
}

/**
 * A stacked row for controls that need the full width (sliders, tables).
 */
export function SettingBlock({ children }: { children: ReactNode }) {
  return <div className="field-row !block">{children}</div>;
}

/**
 * Explanatory note. Reserved for facts about the emulated machine or about
 * where data lives -- things the user cannot change but needs in order to
 * interpret the controls above it.
 */
export function SettingNote({
  title,
  children,
  tone = 'neutral',
}: {
  title?: string;
  children: ReactNode;
  tone?: 'neutral' | 'accent' | 'danger';
}) {
  const toneClass =
    tone === 'accent'
      ? 'border-accent-line bg-accent-soft text-accent-text'
      : tone === 'danger'
        ? 'border-danger-line bg-danger-soft text-danger-text'
        : 'border-line bg-raised text-mute';

  return (
    <div className={`mt-3 rounded-md border px-3 py-2.5 text-[0.8125rem] leading-relaxed ${toneClass}`}>
      {title && (
        <p className="mb-1 flex items-center gap-1.5 font-semibold">
          <IconInfo size={14} />
          {title}
        </p>
      )}
      {children}
    </div>
  );
}
