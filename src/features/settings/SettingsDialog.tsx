import { useEffect, useState } from 'react';
import { useAppStore } from '../../app/store';
import { api } from '../../shared/ipc/commands';
import { formatBytes, useLocale, useT } from '../../shared/i18n';
import { Dialog } from '../../shared/ui/Dialog';
import { ModelSetup } from '../models/ModelSetup';
import './settings.css';

export function SettingsDialog() {
  const t = useT();
  const locale = useLocale();
  const open = useAppStore((s) => s.settingsOpen);
  const setOpen = useAppStore((s) => s.setSettingsOpen);
  const settings = useAppStore((s) => s.settings);
  const storage = useAppStore((s) => s.storage);
  const setLocale = useAppStore((s) => s.setLocale);
  const update = useAppStore((s) => s.updateSettings);
  const fail = useAppStore((s) => s.fail);
  const setOnboardingOpen = useAppStore((s) => s.setOnboardingOpen);
  const [report, setReport] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (open) api.diagnostics().then(setReport).catch(() => setReport(null));
  }, [open]);

  if (!open || !settings) return null;

  async function restartTour() {
    try {
      const ob = await api.restartOnboarding();
      useAppStore.setState({ onboarding: ob });
      setOpen(false);
      setOnboardingOpen(true);
    } catch (e) {
      fail(e);
    }
  }

  return (
    <Dialog title={t('settings.title')} onClose={() => setOpen(false)} width={640}>
      <section className="settings-section">
        <h3>{t('settings.language')}</h3>
        <label className="settings-row">
          <span>{t('settings.interfaceLanguage')}</span>
          <select className="field" value={settings.locale} onChange={(e) => void setLocale(e.target.value === 'ar' ? 'ar' : 'en')}>
            <option value="en">English</option>
            <option value="ar">العربية</option>
          </select>
        </label>
        <label className="settings-row">
          <span>{t('settings.answerLanguage')}</span>
          <select className="field" value={settings.answerLocale} onChange={(e) => void update({ answerLocale: e.target.value })}>
            <option value="auto">{t('settings.answer.auto')}</option>
            <option value="en">English</option>
            <option value="ar">العربية</option>
          </select>
        </label>
        <label className="settings-check">
          <input type="checkbox" checked={settings.enterToSend} onChange={(e) => void update({ enterToSend: e.target.checked })} />
          <span>{settings.enterToSend ? t('settings.enterToSend') : t('settings.enterToSendOff')}</span>
        </label>
      </section>

      <section className="settings-section">
        <h3>{t('settings.models')}</h3>
        <p>{t('settings.modelsBody')}</p>
        <ModelSetup />
      </section>

      <section className="settings-section">
        <h3>{t('settings.storage')}</h3>
        {storage && (
          <dl className="facts">
            <dt>{t('settings.dataDir')}</dt>
            <dd className="selectable mono-small" dir="ltr">
              {storage.dataDir}
            </dd>
            <dt>{t('settings.libraryBytes')}</dt>
            <dd>{formatBytes(locale, storage.libraryBytes)}</dd>
            <dt>{t('settings.databaseBytes')}</dt>
            <dd>{formatBytes(locale, storage.databaseBytes)}</dd>
          </dl>
        )}
        <p className="t-meta">{t('settings.privacy')}</p>
      </section>

      <section className="settings-section">
        <h3>{t('settings.help')}</h3>
        <div>
          <button className="btn" onClick={() => void restartTour()}>
            {t('settings.restartTour')}
          </button>
        </div>
      </section>

      <section className="settings-section">
        <h3>{t('settings.diagnostics')}</h3>
        {report && (
          <pre className="diag selectable" dir="ltr">
            {report}
          </pre>
        )}
        <div className="settings-inline">
          <button
            className="btn"
            disabled={!report}
            onClick={() => {
              if (!report) return;
              void navigator.clipboard.writeText(report).then(() => {
                setCopied(true);
                setTimeout(() => setCopied(false), 2000);
              });
            }}
          >
            {copied ? t('settings.copied') : t('settings.copyReport')}
          </button>
          <span className="t-meta">{t('settings.diagnosticsNote')}</span>
        </div>
      </section>
    </Dialog>
  );
}
