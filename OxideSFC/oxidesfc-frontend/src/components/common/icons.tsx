/**
 * App-level icon set (navigation rail, library, settings).
 *
 * Monochrome 24x24 stroke icons drawn on the same grid and weight as
 * `components/emulator/icons.tsx`, which stays separate because those are
 * transport controls specific to the play deck. Both are hand-rolled rather
 * than pulled from an icon package: a dozen glyphs is not worth a dependency in
 * a binary this project deliberately keeps small.
 *
 * Colour comes from `currentColor`, so every icon follows the accent or text
 * token of whatever it sits inside.
 */

export interface IconProps {
  size?: number;
  className?: string;
}

function stroke(size: number) {
  return {
    width: size,
    height: size,
    viewBox: '0 0 24 24',
    fill: 'none',
    stroke: 'currentColor',
    strokeWidth: 1.7,
    strokeLinecap: 'round' as const,
    strokeLinejoin: 'round' as const,
    'aria-hidden': true,
  };
}

/** The app mark: the Super Famicom's four face buttons in their own colours.
 *  Positioned as they sit on the controller -- X above B, Y left of A. */
export function MarkSFC({ size = 22, className }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      className={className}
      aria-hidden
    >
      <circle cx="12" cy="5.5" r="3.4" fill="var(--sfc-blue)" />
      <circle cx="18.5" cy="12" r="3.4" fill="var(--sfc-red)" />
      <circle cx="12" cy="18.5" r="3.4" fill="var(--sfc-yellow)" />
      <circle cx="5.5" cy="12" r="3.4" fill="var(--sfc-green)" />
    </svg>
  );
}

/** Library: a shelf of cartridges. */
export function IconLibrary({ size = 20, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <rect x="3.5" y="4" width="5" height="16" rx="1.2" />
      <rect x="10.5" y="4" width="5" height="16" rx="1.2" />
      <path d="M18 5.4l3 .8-2.4 13.2-2.4-.7" />
    </svg>
  );
}

export function IconSettings({ size = 20, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.7 1.7 0 0 0 .34 1.87l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.7 1.7 0 0 0-1.87-.34 1.7 1.7 0 0 0-1.03 1.56V21a2 2 0 1 1-4 0v-.09a1.7 1.7 0 0 0-1.11-1.56 1.7 1.7 0 0 0-1.87.34l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.7 1.7 0 0 0 .34-1.87 1.7 1.7 0 0 0-1.56-1.03H3a2 2 0 1 1 0-4h.09a1.7 1.7 0 0 0 1.56-1.11 1.7 1.7 0 0 0-.34-1.87l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.7 1.7 0 0 0 1.87.34h.08a1.7 1.7 0 0 0 1.03-1.56V3a2 2 0 1 1 4 0v.09a1.7 1.7 0 0 0 1.03 1.56 1.7 1.7 0 0 0 1.87-.34l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.7 1.7 0 0 0-.34 1.87v.08a1.7 1.7 0 0 0 1.56 1.03H21a2 2 0 1 1 0 4h-.09a1.7 1.7 0 0 0-1.51 1.03z" />
    </svg>
  );
}

export function IconPlaySolid({ size = 20, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M7 4.5v15l12-7.5L7 4.5z" fill="currentColor" stroke="none" />
    </svg>
  );
}

export function IconSearch({ size = 18, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="M15.5 15.5L21 21" />
    </svg>
  );
}

export function IconStar({
  size = 16,
  className,
  filled = false,
}: IconProps & { filled?: boolean }) {
  return (
    <svg {...stroke(size)} className={className}>
      <path
        d="M12 3.6l2.6 5.3 5.9.85-4.25 4.15 1 5.85L12 17l-5.25 2.75 1-5.85L3.5 9.75l5.9-.85L12 3.6z"
        fill={filled ? 'currentColor' : 'none'}
      />
    </svg>
  );
}

/** Recently played. */
export function IconClock({ size = 16, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3.2 2" />
    </svg>
  );
}

export function IconFolder({ size = 18, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M3.5 7a2 2 0 0 1 2-2h3.3l2 2.5h7.7a2 2 0 0 1 2 2v7.5a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2V7z" />
    </svg>
  );
}

export function IconPlus({ size = 18, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M12 5v14M5 12h14" />
    </svg>
  );
}

export function IconClose({ size = 16, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M6 6l12 12M18 6L6 18" />
    </svg>
  );
}

export function IconCheck({ size = 16, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M4.5 12.5l5 5 10-11" />
    </svg>
  );
}

export function IconChevronDown({ size = 16, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M6 9.5l6 6 6-6" />
    </svg>
  );
}

/** Sort direction marker for table headers. */
export function IconSortAsc({ size = 14, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M12 19V5M6 11l6-6 6 6" />
    </svg>
  );
}

export function IconSortDesc({ size = 14, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M12 5v14M18 13l-6 6-6-6" />
    </svg>
  );
}

export function IconGrid({ size = 18, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <rect x="4" y="4" width="7" height="7" rx="1.5" />
      <rect x="13" y="4" width="7" height="7" rx="1.5" />
      <rect x="4" y="13" width="7" height="7" rx="1.5" />
      <rect x="13" y="13" width="7" height="7" rx="1.5" />
    </svg>
  );
}

export function IconList({ size = 18, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M4 6.5h16M4 12h16M4 17.5h16" />
    </svg>
  );
}

/** Video / output: a CRT-proportioned display. */
export function IconDisplay({ size = 20, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <rect x="3" y="4.5" width="18" height="12" rx="2" />
      <path d="M9 20h6M12 16.5V20" />
    </svg>
  );
}

/** Audio: a speaker with two arcs. */
export function IconAudio({ size = 20, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M4 9.5h2.5L11 5.5v13L6.5 14.5H4a.5.5 0 0 1-.5-.5v-4a.5.5 0 0 1 .5-.5z" />
      <path d="M14.5 9a4.2 4.2 0 0 1 0 6M17.5 6.5a8 8 0 0 1 0 11" />
    </svg>
  );
}

/** Controls: the Super Famicom pad silhouette. */
export function IconGamepad({ size = 20, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M7.5 8h9a4.5 4.5 0 0 1 4.35 3.35l.9 3.4A2.4 2.4 0 0 1 19.4 18c-.9 0-1.72-.5-2.13-1.3L16.6 15.4H7.4l-.67 1.3A2.4 2.4 0 0 1 4.6 18a2.4 2.4 0 0 1-2.35-3.25l.9-3.4A4.5 4.5 0 0 1 7.5 8z" />
      <path d="M6.2 11.6v1.8M5.3 12.5h1.8" />
      <circle cx="16.2" cy="11.9" r="0.85" fill="currentColor" stroke="none" />
      <circle cx="18" cy="13.4" r="0.85" fill="currentColor" stroke="none" />
    </svg>
  );
}

/** Library store: stacked cartridges. */
export function IconDatabase({ size = 20, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <ellipse cx="12" cy="6" rx="7.5" ry="2.8" />
      <path d="M4.5 6v6c0 1.55 3.36 2.8 7.5 2.8s7.5-1.25 7.5-2.8V6" />
      <path d="M4.5 12v6c0 1.55 3.36 2.8 7.5 2.8s7.5-1.25 7.5-2.8v-6" />
    </svg>
  );
}

/** General / appearance. */
export function IconSliders({ size = 20, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M5 5v5M5 14v5M12 5v9M12 18v1M19 5v2M19 11v8" />
      <circle cx="5" cy="12" r="2" />
      <circle cx="12" cy="16" r="2" />
      <circle cx="19" cy="9" r="2" />
    </svg>
  );
}

export function IconInfo({ size = 16, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 11v5.5M12 7.8h.01" />
    </svg>
  );
}

export function IconTrash({ size = 16, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M4.5 7h15M9.5 7V5.2A1.2 1.2 0 0 1 10.7 4h2.6a1.2 1.2 0 0 1 1.2 1.2V7" />
      <path d="M6.5 7l.8 11.3A1.7 1.7 0 0 0 9 20h6a1.7 1.7 0 0 0 1.7-1.7L17.5 7" />
    </svg>
  );
}

export function IconRefresh({ size = 16, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M20 11.5A8 8 0 0 0 6.3 6.3L4 8.5" />
      <path d="M4 4.5v4h4" />
      <path d="M4 12.5A8 8 0 0 0 17.7 17.7L20 15.5" />
      <path d="M20 19.5v-4h-4" />
    </svg>
  );
}

export function IconPencil({ size = 16, className }: IconProps) {
  return (
    <svg {...stroke(size)} className={className}>
      <path d="M15.2 4.8l4 4L8.5 19.5H4.5v-4L15.2 4.8z" />
    </svg>
  );
}
