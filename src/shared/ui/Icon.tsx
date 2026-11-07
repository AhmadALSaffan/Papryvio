// Icons drawn from the Papryvio design files (stroke icons, currentColor).
import type { SVGProps } from 'react';

const paths = {
  import: <path d="M8 2.5v8M4.5 7L8 10.5 11.5 7M3 13h10" />,
  folder: <path d="M1.5 4.5v8a1 1 0 001 1h11a1 1 0 001-1V6a1 1 0 00-1-1H8L6.5 3h-4a1 1 0 00-1 1.5z" />,
  search: (
    <>
      <circle cx="7" cy="7" r="4.5" />
      <path d="M10.5 10.5L14 14" />
    </>
  ),
  collapse: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <path d="M6 2.5v11M10.5 6L8.5 8l2 2" />
    </>
  ),
  expand: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <path d="M6 2.5v11M8.5 6l2 2-2 2" />
    </>
  ),
  settings: (
    <>
      <circle cx="8" cy="8" r="2.2" />
      <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M3.4 12.6l1.4-1.4M11.2 4.8l1.4-1.4" />
    </>
  ),
  check: <path d="M3.3 8.3l3 3 6.4-6.6" />,
  chat: <path d="M2.5 3.5h11a1 1 0 011 1v6a1 1 0 01-1 1H7l-3 2.5v-2.5H2.5a1 1 0 01-1-1v-6a1 1 0 011-1z" />,
  clock: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 4.7V8l2.4 1.6" />
    </>
  ),
  warn: (
    <>
      <path d="M8 2l6.4 11.3H1.6z" />
      <path d="M8 6.7v2.9M8 11.2v.1" />
    </>
  ),
  error: (
    <>
      <circle cx="8" cy="8" r="6.2" />
      <path d="M8 4.8v3.8M8 10.9v.1" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6.2" />
      <path d="M8 7.2v4M8 4.9v.1" />
    </>
  ),
  chevronLeft: <path d="M9.7 3.5L5.2 8l4.5 4.5" />,
  chevronRight: <path d="M6.3 3.5L10.8 8l-4.5 4.5" />,
  chevronDown: <path d="M3.5 6.3L8 10.8l4.5-4.5" />,
  close: <path d="M3.5 3.5l9 9M12.5 3.5l-9 9" />,
  plus: <path d="M8 3v10M3 8h10" />,
  minus: <path d="M3 8h10" />,
  file: (
    <>
      <path d="M4 1.5h5.5L13 5v9.5H4z" />
      <path d="M9.5 1.5V5H13M8.5 7.5v5M6.5 10.5l2 2 2-2" />
    </>
  ),
  eye: (
    <>
      <path d="M1.3 8S3.7 3.4 8 3.4 14.7 8 14.7 8 12.3 12.6 8 12.6 1.3 8 1.3 8z" />
      <circle cx="8" cy="8" r="2" />
    </>
  ),
  pause: <path d="M5.5 3.5v9M10.5 3.5v9" />,
  play: <path d="M5 3.2v9.6L12.5 8z" />,
  edit: <path d="M10.8 2.7l2.5 2.5-7.7 7.7H3.1v-2.5z" />,
  trash: <path d="M2.5 4.5h11M6 4.5V3h4v1.5M4 4.5l.7 9h6.6l.7-9" />,
  open: <path d="M5 3h6.5v6.5M11.5 3L3 11.5" />,
  fit: <path d="M2 5V2h3M11 2h3v3M14 11v3h-3M5 14H2v-3" />,
} as const;

export type IconName = keyof typeof paths;

export function Icon({ name, size = 16, strokeWidth = 1.4, ...rest }: { name: IconName; size?: number; strokeWidth?: number } & SVGProps<SVGSVGElement>) {
  const mirror = name === 'chevronLeft' || name === 'chevronRight';
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      className={mirror ? 'icon-dir' : undefined}
      {...rest}
    >
      {paths[name]}
    </svg>
  );
}
