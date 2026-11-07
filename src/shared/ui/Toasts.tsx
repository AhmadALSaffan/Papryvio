import { useAppStore } from '../../app/store';
import { useT } from '../i18n';
import { Icon } from './Icon';

export function Toasts() {
  const toasts = useAppStore((s) => s.toasts);
  const dismiss = useAppStore((s) => s.dismissToast);
  const t = useT();
  return (
    <div className="toasts" role="status" aria-live="polite">
      {toasts.map((toast) => (
        <div key={toast.id} className={`toast ${toast.kind}`}>
          <Icon name={toast.kind === 'info' ? 'info' : toast.kind === 'warn' ? 'warn' : 'error'} />
          <div>
            {toast.text}
            {toast.reference && <span className="ref">{t('error.reference', { id: toast.reference })}</span>}
          </div>
          <button className="btn btn-ghost btn-icon" style={{ height: 22, width: 22 }} onClick={() => dismiss(toast.id)} aria-label={t('common.dismiss')}>
            <Icon name="close" size={12} />
          </button>
        </div>
      ))}
    </div>
  );
}
