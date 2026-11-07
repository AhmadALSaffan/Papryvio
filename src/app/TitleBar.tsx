import { useEffect, useState } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { useAppStore } from './store';
import { useT } from '../shared/i18n';

/** Custom title bar with real window controls and a drag region. */
export function TitleBar() {
  const t = useT();
  const tab = useAppStore((s) => s.tab);
  const setTab = useAppStore((s) => s.setTab);
  const collection = useAppStore((s) => s.collections.find((c) => c.id === s.activeCollectionId) ?? null);
  const [maximized, setMaximized] = useState(false);

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
