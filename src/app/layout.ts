// Pane sizes, the reader pane's visibility and the interface zoom. These are per-machine view
// preferences, so they live in localStorage rather than in the app database.
import { create } from 'zustand';
import { getCurrentWebview } from '@tauri-apps/api/webview';

export const LIBRARY_W = { min: 200, max: 440, def: 280 };
export const EVIDENCE_W = { min: 320, max: 1100, def: 520 };
export const RAIL_W = 64;
export const CENTER_MIN = 360;
export const ZOOM = { min: 0.8, max: 1.5, step: 0.1, def: 1 };

const KEY = 'papryvio.layout.v1';

interface Saved {
  libraryW: number;
  evidenceW: number;
  evidenceHidden: boolean;
  zoom: number;
}

const DEFAULTS: Saved = { libraryW: LIBRARY_W.def, evidenceW: EVIDENCE_W.def, evidenceHidden: false, zoom: ZOOM.def };

export const clamp = (v: number, min: number, max: number) => Math.min(max, Math.max(min, v));
const roundZoom = (z: number) => Math.round(clamp(z, ZOOM.min, ZOOM.max) * 100) / 100;

function load(): Saved {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS;
    const v = JSON.parse(raw) as Partial<Saved>;
    return {
      libraryW: typeof v.libraryW === 'number' ? clamp(v.libraryW, LIBRARY_W.min, LIBRARY_W.max) : DEFAULTS.libraryW,
      evidenceW: typeof v.evidenceW === 'number' ? clamp(v.evidenceW, EVIDENCE_W.min, EVIDENCE_W.max) : DEFAULTS.evidenceW,
      evidenceHidden: v.evidenceHidden === true,
      zoom: typeof v.zoom === 'number' ? roundZoom(v.zoom) : DEFAULTS.zoom,
    };
  } catch {
    return DEFAULTS;
  }
}

let saveTimer: ReturnType<typeof setTimeout> | null = null;
function save(s: Saved) {
  if (saveTimer) clearTimeout(saveTimer);
  saveTimer = setTimeout(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify(s));
    } catch {
      /* view preferences are optional */
    }
  }, 250);
}

function applyZoom(zoom: number) {
  getCurrentWebview()
    .setZoom(zoom)
    .catch(() => undefined);
}

interface LayoutState extends Saved {
  /** Window width in CSS pixels (already divided by the zoom). */
  viewport: number;
  shortcutsOpen: boolean;
  setLibraryW(w: number): void;
  setEvidenceW(w: number): void;
  resetWidths(): void;
  toggleEvidence(): void;
  setEvidenceHidden(hidden: boolean): void;
  setZoom(z: number): void;
  zoomBy(delta: number): void;
  setViewport(w: number): void;
  setShortcutsOpen(open: boolean): void;
}

const pick = (s: Saved): Saved => ({ libraryW: s.libraryW, evidenceW: s.evidenceW, evidenceHidden: s.evidenceHidden, zoom: s.zoom });

export const useLayoutStore = create<LayoutState>((set, get) => {
  const initial = load();
  if (initial.zoom !== 1) applyZoom(initial.zoom);
  const update = (patch: Partial<Saved>) => {
    set(patch);
    save(pick(get()));
  };
  return {
    ...initial,
    viewport: typeof window === 'undefined' ? 1440 : window.innerWidth,
    shortcutsOpen: false,
    setLibraryW: (w) => update({ libraryW: Math.round(clamp(w, LIBRARY_W.min, LIBRARY_W.max)) }),
    setEvidenceW: (w) => update({ evidenceW: Math.round(clamp(w, EVIDENCE_W.min, EVIDENCE_W.max)) }),
    resetWidths: () => update({ libraryW: LIBRARY_W.def, evidenceW: EVIDENCE_W.def, evidenceHidden: false }),
    toggleEvidence: () => update({ evidenceHidden: !get().evidenceHidden }),
    setEvidenceHidden: (evidenceHidden) => get().evidenceHidden !== evidenceHidden && update({ evidenceHidden }),
    setZoom: (z) => {
      const zoom = roundZoom(z);
      update({ zoom });
      applyZoom(zoom);
    },
    zoomBy: (delta) => get().setZoom(get().zoom + delta),
    setViewport: (viewport) => set({ viewport }),
    setShortcutsOpen: (shortcutsOpen) => set({ shortcutsOpen }),
  };
});

export interface Metrics {
  libraryCollapsed: boolean;
  libraryW: number;
  /** The reader pane sits beside the answer, as a grid column. */
  evidenceColumn: boolean;
  /** The window is too narrow for three columns: the reader slides over the answer instead. */
  evidenceDrawer: boolean;
  evidenceW: number;
}

/** Effective pane sizes for the current window width, keeping the answer column usable. */
export function computeMetrics(
  s: Pick<LayoutState, 'libraryW' | 'evidenceW' | 'evidenceHidden' | 'viewport'>,
  libraryCollapsed: boolean,
  readerOpen: boolean,
): Metrics {
  const libraryW = libraryCollapsed ? RAIL_W : clamp(s.libraryW, LIBRARY_W.min, Math.max(LIBRARY_W.min, s.viewport - CENTER_MIN - RAIL_W));
  const compact = s.viewport < libraryW + EVIDENCE_W.min + CENTER_MIN;
  const evidenceColumn = !compact && !s.evidenceHidden;
  const evidenceDrawer = compact && !s.evidenceHidden && readerOpen;
  const room = compact ? s.viewport - libraryW - 48 : s.viewport - libraryW - CENTER_MIN;
  const evidenceW = clamp(s.evidenceW, Math.min(EVIDENCE_W.min, room), Math.max(Math.min(EVIDENCE_W.min, room), room));
  return { libraryCollapsed, libraryW, evidenceColumn, evidenceDrawer, evidenceW };
}
