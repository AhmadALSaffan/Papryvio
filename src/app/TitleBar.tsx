import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useAppStore } from './store';
import { useLayoutStore } from './layout';
import { Icon } from '../shared/ui/Icon';
import { useT } from '../shared/i18n';

/** Custom title bar with real window controls and a drag region. */
export function TitleBar() {
  const t = useT();
  const tab = useAppStore((s) => s.tab);
  const setTab = useAppStore((s) => s.setTab);
  const collection = useAppStore((s) => s.collections.find((c) => c.id === s.activeCollectionId) ?? null);
  const [maximized, setMaximized] = useState(false);
  const libraryOpen = useAppStore((s) => !s.libraryCollapsed);
  const toggleLibrary = useAppStore((s) => s.toggleLibrary);
  const readerOpen = useLayoutStore((s) => !s.evidenceHidden);
  const toggleReader = useLayoutStore((s) => s.toggleEvidence);
  const zoom = useLayoutStore((s) => s.zoom);
  const setZoom = useLayoutStore((s) => s.setZoom);
  const openShortcuts = useLayoutStore((s) => s.setShortcutsOpen);

  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    void win.isMaximized().then(setMaximized);
    void win
      .onResized(() => {
        void win.isMaximized().then(setMaximized);
      })
      .then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const win = () => getCurrentWindow();
  return (
    <header className="titlebar" data-tauri-drag-region onDoubleClick={(e) => e.target === e.currentTarget && void win().toggleMaximize()}>
      <div className="brand" data-tauri-drag-region>
        <img src="/brand/papryvio-mark.png" alt="" width={22} height={22} />
        <span>{t('app.name')}</span>
      </div>
      <nav className="tabs" role="tablist" aria-label="Papryvio">
        {(['ask', 'compare'] as const).map((key) => (
          <button key={key} role="tab" aria-selected={tab === key} className={`tab${tab === key ? ' active' : ''}`} onClick={() => setTab(key)}>
            {t(key === 'ask' ? 'nav.ask' : 'nav.compare')}
          </button>
        ))}
      </nav>
      <div className="drag-fill" data-tauri-drag-region />
      {collection && (
        <span className="titlebar-context" data-tauri-drag-region dir="auto">
          {collection.name}
        </span>
      )}
      <div className="titlebar-toggles">
        {zoom !== 1 && (
          <button className="zoom-badge" onClick={() => setZoom(1)} title={t('layout.zoomReset')}>
            {Math.round(zoom * 100)}%
          </button>
        )}
        <button onClick={() => openShortcuts(true)} title={`${t('shortcuts.title')} (Ctrl+/)`} aria-label={t('shortcuts.title')}>
          <Icon name="keyboard" size={15} />
        </button>
        <button onClick={toggleLibrary} aria-pressed={libraryOpen} title={`${t('layout.toggleLibrary')} (Ctrl+B)`} aria-label={t('layout.toggleLibrary')}>
          <Icon name="panelStart" size={15} />
        </button>
        <button onClick={toggleReader} aria-pressed={readerOpen} title={`${t('layout.toggleReader')} (Ctrl+J)`} aria-label={t('layout.toggleReader')}>
          <Icon name="panelEnd" size={15} />
        </button>
      </div>
      <div className="window-controls">
        <button onClick={() => void win().minimize()} aria-label={t('window.minimize')} title={t('window.minimize')}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M0 5h10" stroke="currentColor" />
          </svg>
        </button>
        <button
          onClick={() => void win().toggleMaximize()}
          aria-label={maximized ? t('window.restore') : t('window.maximize')}
          title={maximized ? t('window.restore') : t('window.maximize')}
        >
          {maximized ? (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <rect x="0.5" y="2.5" width="7" height="7" fill="none" stroke="currentColor" />
              <path d="M2.5 2.5V.5h7v7h-2" fill="none" stroke="currentColor" />
            </svg>
          ) : (
            <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
              <rect x=".5" y=".5" width="9" height="9" fill="none" stroke="currentColor" />
            </svg>
          )}
        </button>
        <button className="close" onClick={() => void win().close()} aria-label={t('window.close')} title={t('window.close')}>
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
            <path d="M0 0l10 10M10 0L0 10" stroke="currentColor" />
          </svg>
        </button>
      </div>
    </header>
  );
}
