import { useEffect, type CSSProperties } from 'react';
import { Onboarding } from '../features/onboarding/Onboarding';
import { SettingsDialog } from '../features/settings/SettingsDialog';
import { useT } from '../shared/i18n';
import { Icon } from '../shared/ui/Icon';
import { Toasts } from '../shared/ui/Toasts';
import { AppDialogs } from './AppDialogs';
import { useLayoutStore } from './layout';
import { Shell, useMetrics } from './Shell';
import { ShortcutsDialog } from './ShortcutsDialog';
import { useAppStore } from './store';
import { useRagStore } from './ragStore';
import { TitleBar } from './TitleBar';
import { useShortcuts } from './useShortcuts';
import './shell.css';

export function App() {
  const ready = useAppStore((s) => s.ready);
  const bootError = useAppStore((s) => s.bootError);
  const locale = useAppStore((s) => s.locale);
  const metrics = useMetrics();
  const init = useAppStore((s) => s.init);
  const t = useT();
  useShortcuts();

  useEffect(() => {
    void init().then(() => useRagStore.getState().init());
  }, [init]);

  useEffect(() => {
    const onResize = () => useLayoutStore.getState().setViewport(window.innerWidth);
    onResize();
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  // The chrome direction follows the interface language.
  useEffect(() => {
    document.documentElement.lang = locale;
    document.documentElement.dir = locale === 'ar' ? 'rtl' : 'ltr';
  }, [locale]);

  if (bootError) {
    return (
      <div className="boot-error" role="alert">
        <Icon name="error" size={20} />
        <p>{t(bootError.key)}</p>
        {bootError.reference && <p className="t-meta">{t('error.reference', { id: bootError.reference })}</p>}
      </div>
    );
  }

  return (
    <div
      className={`app${metrics.libraryCollapsed ? ' library-collapsed' : ''}`}
      style={{ '--library-w': `${metrics.libraryW}px` } as CSSProperties}
    >
      <TitleBar />
      {ready && <Shell />}
      <DropOverlay />
      <AppDialogs />
      <SettingsDialog />
      <ShortcutsDialog />
      <Onboarding />
      <Toasts />
      <LiveRegion />
    </div>
  );
}

function DropOverlay() {
  const t = useT();
  const active = useAppStore((s) => s.dragActive);
  const total = useAppStore((s) => s.documentsTotal);
  const collection = useAppStore((s) => s.collections.find((c) => c.id === s.activeCollectionId) ?? null);
  if (!active || total === 0) return null;
  return (
    <div className="drop-overlay" aria-hidden="true">
      <div>
        <Icon name="import" size={28} />
        <strong>{t('drop.title')}</strong>
        {collection && <span dir="auto">{t('drop.into', { name: collection.name })}</span>}
      </div>
    </div>
  );
}

/** Announces processing changes to screen readers without flooding them with every page. */
function LiveRegion() {
  const t = useT();
  const processing = useAppStore((s) => s.documents.filter((d) => d.state === 'imported' || d.state === 'extracting').length);
  const total = useAppStore((s) => s.documentsTotal);
  const ready = useAppStore((s) => s.documents.filter((d) => d.lexicalReady || d.state === 'text_ready').length);
  return (
    <div className="sr-only" aria-live="polite">
      {total > 0 ? (processing > 0 ? t('library.footerProcessing', { ready, total, processing }) : t('library.footer', { ready, total })) : ''}
    </div>
  );
}
