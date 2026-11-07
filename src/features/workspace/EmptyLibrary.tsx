import { useAppStore } from '../../app/store';
import { useT } from '../../shared/i18n';
import { Icon } from '../../shared/ui/Icon';

export function EmptyLibrary() {
  const t = useT();
  const startImport = useAppStore((s) => s.startImport);
  const dragActive = useAppStore((s) => s.dragActive);
  return (
    <div className="empty">
      <div className="empty-intro">
        <h1>{t('empty.title')}</h1>
        <p className="t-body">{t('empty.body')}</p>
      </div>
      <div className={`dropzone${dragActive ? ' active' : ''}`}>
        <Icon name="file" size={28} strokeWidth={1.1} />
        <span className="dropzone-title">{t('empty.drop')}</span>
        <button className="btn btn-primary btn-lg" onClick={() => void startImport()}>
          {t('empty.choose')}
        </button>
        <span className="dropzone-hint">{t('empty.hint')}</span>
      </div>
      <div className="after-import">
        <span className="t-label">{t('empty.after')}</span>
        <ol>
          <li>{t('empty.step1')}</li>
          <li>{t('empty.step2')}</li>
          <li>{t('empty.step3')}</li>
        </ol>
        <p className="t-meta">{t('empty.copyNote')}</p>
      </div>
    </div>
  );
}
