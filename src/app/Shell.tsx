import { useEffect, useRef, type CSSProperties } from 'react';
import { EvidencePane } from '../features/reader/EvidencePane';
import { LibrarySidebar } from '../features/library/LibrarySidebar';
import { Workspace } from '../features/workspace/Workspace';
import { useT } from '../shared/i18n';
import { CENTER_MIN, EVIDENCE_W, LIBRARY_W, computeMetrics, useLayoutStore } from './layout';
import { useAppStore } from './store';

/** Effective layout for the current window, shared by the shell and the title bar. */
export function useMetrics() {
  const collapsed = useAppStore((s) => s.libraryCollapsed);
  const readerOpen = useAppStore((s) => !!s.reader);
  const libraryW = useLayoutStore((s) => s.libraryW);
  const evidenceW = useLayoutStore((s) => s.evidenceW);
  const evidenceHidden = useLayoutStore((s) => s.evidenceHidden);
  const viewport = useLayoutStore((s) => s.viewport);
  return computeMetrics({ libraryW, evidenceW, evidenceHidden, viewport }, collapsed, readerOpen);
}

/** Library · answer · reader, with draggable dividers between them. */
export function Shell() {
  const t = useT();
  const m = useMetrics();
  const reader = useAppStore((s) => s.reader);
  const viewport = useLayoutStore((s) => s.viewport);
  const { setLibraryW, setEvidenceW, setEvidenceHidden } = useLayoutStore.getState();

  // Opening a page (a citation, "Open in reader") brings the reader pane back.
  useEffect(() => {
    if (reader) setEvidenceHidden(false);
  }, [reader, setEvidenceHidden]);

  const columns = [`${m.libraryW}px`, 'minmax(0, 1fr)', m.evidenceColumn ? `${m.evidenceW}px` : null].filter(Boolean).join(' ');
  const showEvidence = m.evidenceColumn || m.evidenceDrawer;
  return (
    <div className={`shell${m.evidenceDrawer ? ' with-drawer' : ''}`} style={{ gridTemplateColumns: columns } as CSSProperties}>
      <LibrarySidebar />
      <Workspace />
      {m.evidenceDrawer && <div className="drawer-scrim" aria-hidden="true" onClick={() => setEvidenceHidden(true)} />}
      {showEvidence && <EvidencePane drawer={m.evidenceDrawer} width={m.evidenceW} />}
      {!m.libraryCollapsed && (
        <Splitter
          edge="start"
          offset={m.libraryW}
          value={m.libraryW}
          min={LIBRARY_W.min}
          max={Math.min(LIBRARY_W.max, viewport - CENTER_MIN - (m.evidenceColumn ? m.evidenceW : 0))}
          def={LIBRARY_W.def}
          onChange={setLibraryW}
          label={t('layout.resizeLibrary')}
        />
      )}
      {showEvidence && (
        <Splitter
          edge="end"
          offset={m.evidenceW}
          value={m.evidenceW}
          min={EVIDENCE_W.min}
          max={m.evidenceDrawer ? viewport - m.libraryW - 48 : Math.min(EVIDENCE_W.max, viewport - m.libraryW - CENTER_MIN)}
          def={EVIDENCE_W.def}
          onChange={setEvidenceW}
          label={t('layout.resizeReader')}
          floating={m.evidenceDrawer}
        />
      )}
    </div>
  );
}

/**
 * A vertical divider. `edge` says which side of the window its pane is anchored to (inline start for the
 * library, inline end for the reader), so dragging and the arrow keys follow the reading direction.
 * Double-click or Enter restores the default width.
 */
function Splitter({
  edge,
  offset,
  value,
  min,
  max,
  def,
  onChange,
  label,
  floating,
}: {
  edge: 'start' | 'end';
  offset: number;
  value: number;
  min: number;
  max: number;
  def: number;
  onChange: (w: number) => void;
  label: string;
  floating?: boolean;
}) {
  const drag = useRef<{ x: number; start: number } | null>(null);
  const hi = Math.max(min, max);
  const set = (w: number) => onChange(Math.min(hi, Math.max(min, w)));
  // +1 when moving the pointer right widens this pane.
  const sign = () => {
    const rtl = document.documentElement.dir === 'rtl';
    return (edge === 'start' ? 1 : -1) * (rtl ? -1 : 1);
  };

  function onPointerDown(e: React.PointerEvent<HTMLDivElement>) {
    if (e.button !== 0) return;
    e.preventDefault();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = { x: e.clientX, start: value };
    document.body.classList.add('resizing');
  }
  function onPointerMove(e: React.PointerEvent<HTMLDivElement>) {
    const d = drag.current;
    if (!d) return;
    set(d.start + (e.clientX - d.x) * sign());
  }
  function end(e: React.PointerEvent<HTMLDivElement>) {
    if (!drag.current) return;
    drag.current = null;
    e.currentTarget.releasePointerCapture?.(e.pointerId);
    document.body.classList.remove('resizing');
  }
  function onKeyDown(e: React.KeyboardEvent<HTMLDivElement>) {
    const step = e.shiftKey ? 64 : 16;
    if (e.key === 'ArrowLeft' || e.key === 'ArrowRight') {
      e.preventDefault();
      set(value + (e.key === 'ArrowRight' ? step : -step) * sign());
    } else if (e.key === 'Home') {
      e.preventDefault();
      set(min);
    } else if (e.key === 'End') {
      e.preventDefault();
      set(hi);
    } else if (e.key === 'Enter') {
      e.preventDefault();
      set(def);
    }
  }

  const style: CSSProperties = edge === 'start' ? { insetInlineStart: offset - 4 } : { insetInlineEnd: offset - 4 };
  return (
    <div
      className={`splitter${floating ? ' floating' : ''}`}
      style={style}
      role="separator"
      aria-orientation="vertical"
      aria-label={label}
      aria-valuenow={Math.round(value)}
      aria-valuemin={min}
      aria-valuemax={Math.round(hi)}
      tabIndex={0}
      title={label}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={end}
      onPointerCancel={end}
      onDoubleClick={() => set(def)}
      onKeyDown={onKeyDown}
    />
  );
}
