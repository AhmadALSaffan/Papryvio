import { useEffect, useRef, useState } from 'react';
import { useAppStore } from '../../app/store';
import { formatNumber, useLocale, useT } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';
import type { DocumentView } from '../../shared/types/generated/DocumentView';
import { docStatus, hasText, isProcessing, metaLine } from './status';
import './library.css';

export function LibrarySidebar() {
  const collapsed = useAppStore((s) => s.libraryCollapsed);
  return collapsed ? <LibraryRail /> : <LibraryPanel />;
}

function useCounts() {
  const docs = useAppStore((s) => s.documents);
  const total = useAppStore((s) => s.documentsTotal);
  const filter = useAppStore((s) => s.filter);
  const ready = docs.filter((d) => d.lexicalReady || hasText(d)).length;
  const processing = docs.filter(isProcessing).length;
  return { docs, total, ready, processing, filtered: filter.trim().length > 0 };
}

function LibraryRail() {
  const t = useT();
  const locale = useLocale();
  const toggle = useAppStore((s) => s.toggleLibrary);
  const startImport = useAppStore((s) => s.startImport);
  const openSettings = useAppStore((s) => s.setSettingsOpen);
  const { ready, total } = useCounts();
  return (
    <nav className="library rail" aria-label={t('library.heading')} data-region="library" tabIndex={-1}>
      <button className="rail-btn" onClick={toggle} title={t('library.expand')} aria-label={t('library.expand')} data-coach="library-toggle">
        <Icon name="expand" size={18} strokeWidth={1.3} />
      </button>
      <span className="rail-label">{t('library.heading')}</span>
      <button className="rail-import" onClick={() => void startImport()} title={`${t('library.import')} (Ctrl+O)`} aria-label={t('library.import')}>
        <Icon name="import" size={18} strokeWidth={1.6} />
      </button>
      <span className="rail-label strong">{t('library.importShort')}</span>
      <div className="rail-sep" />
      <span className="rail-count">
        {formatNumber(locale, ready)}/{formatNumber(locale, total)}
      </span>
      <span className="rail-label">{t('library.rail.withText')}</span>
      <div style={{ flex: 1 }} />
      <button className="rail-btn" onClick={() => openSettings(true)} title={t('library.settings')} aria-label={t('library.settings')}>
        <Icon name="settings" size={16} strokeWidth={1.3} />
      </button>
    </nav>
  );
}

function LibraryPanel() {
  const t = useT();
  const locale = useLocale();
  const toggle = useAppStore((s) => s.toggleLibrary);
  const startImport = useAppStore((s) => s.startImport);
  const importing = useAppStore((s) => s.importing);
  const filter = useAppStore((s) => s.filter);
  const setFilter = useAppStore((s) => s.setFilter);
  const openSettings = useAppStore((s) => s.setSettingsOpen);
  const { docs, total, ready, processing, filtered } = useCounts();
  const hasAnyDocs = total > 0 || filtered;

  const footer =
    total === 0 && !filtered
      ? t('library.footerEmpty')
      : processing > 0
        ? t('library.footerProcessing', { ready, total, processing })
        : t('library.footer', { ready, total });

  return (
    <nav className="library" aria-label={t('library.heading')} data-region="library" tabIndex={-1}>
      <div className="library-top">
        <div className="library-head">
          <span className="library-title">{t('library.heading')}</span>
          <button className="icon-btn" onClick={toggle} title={t('library.collapse')} aria-label={t('library.collapse')} data-coach="library-toggle">
            <Icon name="collapse" size={16} strokeWidth={1.3} />
          </button>
        </div>
        <button className="import-btn" onClick={() => void startImport()} disabled={importing} data-coach="import" id="import-button">
          <Icon name="import" size={16} strokeWidth={1.6} />
          {t('library.import')}
          <span className="kbd">Ctrl+O</span>
        </button>
        <CollectionSwitcher />
        {hasAnyDocs && (
          <label className="filter">
            <Icon name="search" size={14} />
            <span className="sr-only">{t('library.filter')}</span>
            <input value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t('library.filter')} type="search" />
          </label>
        )}
      </div>
      <div className="library-list-head">
        <span className="t-label">{t('library.papers')}</span>
        {total > 0 && <span className="t-meta">{formatNumber(locale, total)}</span>}
      </div>
      {!hasAnyDocs && (
        <div className="library-empty">
          <strong>{t('library.emptyTitle')}</strong>
          <span>{t('library.emptyBody')}</span>
        </div>
      )}
      {filtered && docs.length === 0 && <div className="library-empty plain">{t('library.noMatches', { filter })}</div>}
      <DocumentList docs={docs} />
      <div className="library-foot">
        <span aria-live="polite">{footer}</span>
        <button className="foot-btn" onClick={() => openSettings(true)}>
          <Icon name="settings" size={14} strokeWidth={1.3} />
          {t('library.settings')}
        </button>
      </div>
    </nav>
  );
}

function DocumentList({ docs }: { docs: DocumentView[] }) {
  const listRef = useRef<HTMLUListElement>(null);
  function onKeyDown(e: React.KeyboardEvent) {
    if (e.key !== 'ArrowDown' && e.key !== 'ArrowUp') return;
    const items = Array.from(listRef.current?.querySelectorAll<HTMLButtonElement>('button.row') ?? []);
    const i = items.indexOf(document.activeElement as HTMLButtonElement);
    const next = items[e.key === 'ArrowDown' ? Math.min(items.length - 1, i + 1) : Math.max(0, i - 1)];
    if (next) {
      e.preventDefault();
      next.focus();
    }
  }
  return (
    <ul className="library-list scroll" ref={listRef} onKeyDown={onKeyDown} data-coach="library-list">
      {docs.map((d) => (
        <li key={d.id}>
          <LibraryRow doc={d} />
        </li>
      ))}
    </ul>
  );
}

function LibraryRow({ doc }: { doc: DocumentView }) {
  const t = useT();
  const locale = useLocale();
  const selected = useAppStore((s) => s.selectedDocumentId === doc.id);
  const reader = useAppStore((s) => s.reader);
  const select = useAppStore((s) => s.selectDocument);
  const openReader = useAppStore((s) => s.openReader);
  const status = docStatus(doc);
  const viewingPage = reader?.documentId === doc.id ? reader.page : null;
  const meta = metaLine(doc, (n) => t('doc.pages', { count: n }));

  return (
    <button
      className={`row${selected ? ' selected' : ''}`}
      aria-current={selected ? 'true' : undefined}
      onClick={() => select(doc.id)}
      onDoubleClick={() => openReader(doc.id)}
      onKeyDown={(e) => {
        if (e.key === 'Enter' && e.shiftKey) {
          e.preventDefault();
          openReader(doc.id);
        }
      }}
    >
      {selected && <span className="row-marker" aria-hidden="true" />}
      <span className="row-title" dir="auto">
        {doc.title}
      </span>
      {meta && (
        <span className="row-meta" dir="auto">
          {meta}
        </span>
      )}
      <RowStatus status={status} viewingPage={viewingPage} locale={locale} t={t} semantic={doc.semanticReady} />
    </button>
  );
}

function RowStatus({
  status,
  viewingPage,
  locale,
  t,
  semantic,
}: {
  semantic: boolean;
  status: ReturnType<typeof docStatus>;
  viewingPage: number | null;
  locale: ReturnType<typeof useLocale>;
  t: ReturnType<typeof useT>;
}) {
  if (status.kind === 'progress') {
    const pct = status.fraction !== null ? Math.round(status.fraction * 100) : null;
    return (
      <span className="row-progress">
        <span className="row-progress-line">
          <span>{t(status.label)}</span>
          <span className="mono">{status.total ? `${formatNumber(locale, status.done ?? 0)}/${formatNumber(locale, status.total)}` : ''}</span>
        </span>
        <span
          className={`progress${pct === null ? ' indeterminate' : ''}`}
          role="progressbar"
          aria-valuemin={0}
          aria-valuemax={100}
          {...(pct !== null ? { 'aria-valuenow': pct } : {})}
        >
          <span style={pct !== null ? { width: `${pct}%` } : undefined} />
        </span>
      </span>
    );
  }
  const icon = status.kind === 'ok' ? 'check' : status.kind === 'warn' ? 'warn' : status.kind === 'error' ? 'error' : 'clock';
  return (
    <span className={`status ${status.kind}`}>
      <Icon name={icon} size={12} strokeWidth={1.6} />
      {t(status.label)}
      {semantic && <span className="t-meta"> · {t('status.meaning')}</span>}
      {viewingPage !== null && (
        <span className="viewing">
          {' · '}
          {t('status.viewing')} <span className="mono">p. {formatNumber(locale, viewingPage)}</span>
        </span>
      )}
    </span>
  );
}

function CollectionSwitcher() {
  const t = useT();
  const locale = useLocale();
  const collections = useAppStore((s) => s.collections);
  const activeId = useAppStore((s) => s.activeCollectionId);
  const select = useAppStore((s) => s.selectCollection);
  const setDialog = useAppStore((s) => s.setDialog);
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const active = collections.find((c) => c.id === activeId) ?? null;
  const totalAll = useAppStore((s) => (s.activeCollectionId === null ? s.documentsTotal : null));

  useEffect(() => {
    if (!open) return;
    const close = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [open]);

  const countLabel = (n: number) => (n === 1 ? t('library.papersCountOne') : t('library.papersCount', { count: n }));

  if (collections.length === 0) {
    return (
      <button className="collection-chip" onClick={() => setDialog({ kind: 'createCollection' })} data-coach="collection">
        <span className="chip-name">
          <Icon name="plus" size={14} />
          {t('library.noCollection')}
        </span>
        <span className="t-meta">{countLabel(0)}</span>
      </button>
    );
  }

  return (
    <div className="collection-wrap" ref={wrapRef} data-coach="collection">
      <button
        className="collection-chip"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-label={t('library.chooseCollection')}
        onClick={() => setOpen((o) => !o)}
        onKeyDown={(e) => e.key === 'Escape' && setOpen(false)}
      >
        <span className="chip-name">
          <Icon name="folder" size={14} strokeWidth={1.3} />
          <span className="ellipsis" dir="auto">
            {active ? active.name : t('library.allPapers')}
          </span>
        </span>
        <span className="t-meta">{countLabel(active ? active.documentCount : (totalAll ?? 0))}</span>
      </button>
      {open && (
        <div className="menu" role="menu" style={{ insetInlineStart: 0, top: 36, width: '100%' }} onKeyDown={(e) => e.key === 'Escape' && setOpen(false)}>
          <button className="menu-item" role="menuitemradio" aria-checked={activeId === null} onClick={() => (setOpen(false), void select(null))}>
            {t('library.allPapers')}
          </button>
          {collections.map((c) => (
            <button key={c.id} className="menu-item" role="menuitemradio" aria-checked={c.id === activeId} onClick={() => (setOpen(false), void select(c.id))}>
              <span className="ellipsis" dir="auto">
                {c.name}
              </span>
              <span className="count">{formatNumber(locale, c.documentCount)}</span>
            </button>
          ))}
          <div className="menu-sep" />
          <button className="menu-item" role="menuitem" onClick={() => (setOpen(false), setDialog({ kind: 'createCollection' }))}>
            {t('library.newCollection')}
          </button>
          {active && (
            <>
              <button className="menu-item" role="menuitem" onClick={() => (setOpen(false), setDialog({ kind: 'renameCollection', id: active.id, name: active.name }))}>
                {t('library.renameCollection')}
              </button>
              <button className="menu-item danger" role="menuitem" onClick={() => (setOpen(false), setDialog({ kind: 'deleteCollection', id: active.id, name: active.name }))}>
                {t('library.deleteCollection')}
              </button>
            </>
          )}
        </div>
      )}
    </div>
  );
}
