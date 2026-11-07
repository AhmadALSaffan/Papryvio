import { useCallback, useEffect, useRef, useState } from 'react';
import { useAppStore } from '../../app/store';
import { useRagStore } from '../../app/ragStore';
import { formatNumber, useLocale, useT } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import { PdfViewer, type Zoom } from './PdfViewer';
import './reader.css';

export function EvidencePane({ drawer = false, width }: { drawer?: boolean; width?: number }) {
  const t = useT();
  const reader = useAppStore((s) => s.reader);
  return (
    <aside
      className={`evidence${drawer ? ' drawer' : ''}`}
      style={drawer && width ? { width } : undefined}
      data-region="evidence"
      tabIndex={-1}
      aria-label={t('reader.title')}
    >
      {reader ? (
        <Reader key={reader.documentId} documentId={reader.documentId} page={reader.page} />
      ) : (
        <>
          <div className="pane-head">
            <span className="pane-title">{t('reader.title')}</span>
          </div>
          <div className="evidence-empty">
            <span>{t('reader.placeholder')}</span>
          </div>
        </>
      )}
    </aside>
  );
}

const ZOOM_STEPS = [0.5, 0.67, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3];

function Reader({ documentId, page }: { documentId: string; page: number }) {
  const t = useT();
  const locale = useLocale();
  const doc = useAppStore((s) => s.documents.find((d) => d.id === documentId) ?? null);
  const setPage = useAppStore((s) => s.setReaderPage);
  const closeReader = useAppStore((s) => s.closeReader);
  const evidence = useRagStore((s) => s.evidence);
  const closeEvidence = useRagStore((s) => s.closeEvidence);
  const stepEvidence = useRagStore((s) => s.stepEvidence);
  const [pageCount, setPageCount] = useState<number>(doc?.pageCount ?? 0);
  const [zoom, setZoom] = useState<Zoom>('fit');
  const [scale, setScale] = useState(1);
  const [pageInput, setPageInput] = useState(String(page));
  const opener = useRef<HTMLElement | null>(null);

  const ev = evidence && evidence.location.documentId === documentId ? evidence : null;
  const onEvidencePage = !!ev && ev.location.pageIndex + 1 === page;

  useEffect(() => {
    opener.current = document.activeElement as HTMLElement | null;
  }, []);
  useEffect(() => setPageInput(String(page)), [page]);

  const close = useCallback(() => {
    if (useRagStore.getState().evidence) {
      closeEvidence(); // returns focus to the citation that opened the page
      return;
    }
    const target = opener.current;
    closeReader();
    requestAnimationFrame(() => target?.isConnected && target.focus());
  }, [closeEvidence, closeReader]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (document.querySelector('.dialog-backdrop')) return;
      const inEvidence = (document.activeElement as HTMLElement | null)?.closest('[data-region="evidence"]');
      const typing = ['INPUT', 'TEXTAREA', 'SELECT'].includes((document.activeElement as HTMLElement | null)?.tagName ?? '');
      if (e.key === 'Escape' && !e.defaultPrevented) {
        e.preventDefault();
        close();
      } else if (inEvidence && !typing && (e.key === '[' || e.key === ']') && useRagStore.getState().evidence) {
        e.preventDefault();
        void stepEvidence(e.key === ']' ? 1 : -1);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [close, stepEvidence]);

  const go = (n: number) => {
    if (pageCount === 0) return;
    setPage(Math.min(Math.max(1, n), pageCount));
  };
  const step = (dir: 1 | -1) => {
    const current = zoom === 'fit' ? scale : zoom;
    const next = dir > 0 ? ZOOM_STEPS.find((z) => z > current + 0.01) : [...ZOOM_STEPS].reverse().find((z) => z < current - 0.01);
    if (next) setZoom(next);
  };
  const onZoomResolved = useCallback((s: number) => setScale(s), []);
  const meta = [doc?.authors, doc?.year ? String(doc.year) : null].filter(Boolean).join(' · ');
  const title = doc?.title ?? ev?.location.title ?? '';

  return (
    <>
      <div className="pane-head">
        <button className="btn" onClick={close}>
          <Icon name="chevronLeft" size={14} strokeWidth={1.5} />
          {ev ? t('reader.backToAnswer') : t('reader.back')} <span className="kbd">Esc</span>
        </button>
        {ev && ev.nav.length > 0 && (
          <span className="pane-counter">{t('reader.citationOf', { n: ev.index + 1, total: ev.nav.length })}</span>
        )}
        <div className="pager">
          {ev ? (
            <>
              <button className="btn btn-icon" onClick={() => void stepEvidence(-1)} disabled={ev.index <= 0} aria-label={t('reader.prevCitation')} title={`${t('reader.prevCitation')} ([)`}>
                <Icon name="chevronLeft" size={14} strokeWidth={1.5} />
              </button>
              <button
                className="btn btn-icon"
                onClick={() => void stepEvidence(1)}
                disabled={ev.index >= ev.nav.length - 1}
                aria-label={t('reader.nextCitation')}
                title={`${t('reader.nextCitation')} (])`}
              >
                <Icon name="chevronRight" size={14} strokeWidth={1.5} />
              </button>
            </>
          ) : (
            <>
              <button className="btn btn-icon" onClick={() => go(page - 1)} disabled={page <= 1} aria-label={t('reader.prev')} title={t('reader.prev')}>
                <Icon name="chevronLeft" size={14} strokeWidth={1.5} />
              </button>
              <button className="btn btn-icon" onClick={() => go(page + 1)} disabled={pageCount === 0 || page >= pageCount} aria-label={t('reader.next')} title={t('reader.next')}>
                <Icon name="chevronRight" size={14} strokeWidth={1.5} />
              </button>
            </>
          )}
        </div>
      </div>
      <div className="reader-band">
        <div className="reader-doc">
          <span className="reader-title" dir="auto">
            {title}
          </span>
          {meta && (
            <span className="t-meta" dir="auto">
              {meta}
            </span>
          )}
        </div>
        <div className="reader-tools">
          <form
            className="page-box"
            onSubmit={(e) => {
              e.preventDefault();
              const n = parseInt(pageInput, 10);
              if (Number.isFinite(n)) go(n);
              else setPageInput(String(page));
            }}
          >
            <label>
              <span className="sr-only">{t('reader.page')}</span>
              <span aria-hidden="true">p.</span>
              <input
                value={pageInput}
                onChange={(e) => setPageInput(e.target.value.replace(/[^0-9]/g, ''))}
                onBlur={() => setPageInput(String(page))}
                inputMode="numeric"
                size={Math.max(2, String(pageCount).length)}
              />
            </label>
            <span>/ {pageCount ? formatNumber(locale, pageCount) : '–'}</span>
          </form>
          {ev && (
            <>
              <button className="btn btn-icon" onClick={() => go(page - 1)} disabled={page <= 1} aria-label={t('reader.prev')} title={t('reader.prev')}>
                <Icon name="chevronLeft" size={12} strokeWidth={1.5} />
              </button>
              <button className="btn btn-icon" onClick={() => go(page + 1)} disabled={pageCount === 0 || page >= pageCount} aria-label={t('reader.next')} title={t('reader.next')}>
                <Icon name="chevronRight" size={12} strokeWidth={1.5} />
              </button>
            </>
          )}
          <button className="btn btn-icon" onClick={() => step(-1)} aria-label={t('reader.zoomOut')} title={t('reader.zoomOut')}>
            <Icon name="minus" size={14} />
          </button>
          <span className="zoom-value mono">{formatNumber(locale, Math.round(scale * 100))}%</span>
          <button className="btn btn-icon" onClick={() => step(1)} aria-label={t('reader.zoomIn')} title={t('reader.zoomIn')}>
            <Icon name="plus" size={14} />
          </button>
          <button className="btn" aria-pressed={zoom === 'fit'} onClick={() => setZoom('fit')}>
            {t('reader.fitWidth')}
          </button>
        </div>
        {ev && ev.location.boxes.length > 0 && (
          <div className="evidence-legend" data-coach="evidence-legend">
            <span className="legend-swatch" />
            {t('reader.exactPassage', { n: ev.number })}
            {!onEvidencePage && (
              <button className="link-btn" onClick={() => go(ev.location.pageIndex + 1)}>
                {t('reader.backToPassage', { page: ev.location.pageIndex + 1 })}
              </button>
            )}
          </div>
        )}
        {ev && ev.location.boxes.length === 0 && (
          <>
            <span className="status neutral">
              <Icon name="info" size={12} />
              {ev.location.deleted ? t('reader.sourceDeleted') : t('reader.notLocated', { page: ev.location.pageIndex + 1 })}
            </span>
            <p className="evidence-excerpt selectable" dir="auto">
              “{ev.location.quote ?? ev.location.excerpt.slice(0, 400)}”
            </p>
          </>
        )}
        {doc?.state === 'needs_ocr' && <span className="t-meta">{t('reader.readOnlyScan')}</span>}
      </div>
      {ev?.location.deleted ? (
        <div className="evidence-empty">
          <span>{t('reader.sourceDeleted')}</span>
        </div>
      ) : (
        <PdfViewer
          documentId={documentId}
          page={page}
          zoom={zoom}
          onPageCount={setPageCount}
          onPageChange={go}
          onZoomResolved={onZoomResolved}
          highlights={onEvidencePage ? ev!.location.boxes : []}
          marker={onEvidencePage ? ev!.number : undefined}
        />
      )}
    </>
  );
}
