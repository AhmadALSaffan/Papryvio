import { useT, type MessageKey } from '../shared/i18n';
import { Dialog } from '../shared/ui/Dialog';
import { useLayoutStore } from './layout';

const GROUPS: { title: MessageKey; rows: [string[], MessageKey][] }[] = [
  {
    title: 'shortcuts.groupGeneral',
    rows: [
      [['Ctrl', 'O'], 'shortcuts.import'],
      [['Ctrl', 'N'], 'shortcuts.newQuestion'],
      [['Enter'], 'shortcuts.send'],
      [['Shift', 'Enter'], 'shortcuts.newLine'],
      [['Ctrl', ','], 'shortcuts.settings'],
      [['Ctrl', '/'], 'shortcuts.list'],
      [['F1'], 'shortcuts.list'],
    ],
  },
  {
    title: 'shortcuts.groupLayout',
    rows: [
      [['Ctrl', 'B'], 'shortcuts.library'],
      [['Ctrl', 'J'], 'shortcuts.reader'],
      [['Ctrl', '+'], 'shortcuts.zoomIn'],
      [['Ctrl', '−'], 'shortcuts.zoomOut'],
      [['Ctrl', '0'], 'shortcuts.zoomReset'],
      [['F6'], 'shortcuts.regions'],
      [['←', '→'], 'shortcuts.divider'],
    ],
  },
  {
    title: 'shortcuts.groupReader',
    rows: [
      [[']'], 'shortcuts.nextSource'],
      [['['], 'shortcuts.prevSource'],
      [['Esc'], 'shortcuts.back'],
    ],
  },
];

/** Every keyboard shortcut in one place (Ctrl+/). */
export function ShortcutsDialog() {
  const t = useT();
  const open = useLayoutStore((s) => s.shortcutsOpen);
  const setOpen = useLayoutStore((s) => s.setShortcutsOpen);
  if (!open) return null;
  return (
    <Dialog title={t('shortcuts.title')} onClose={() => setOpen(false)} width={520}>
      <div className="shortcuts">
        {GROUPS.map((g) => (
          <section key={g.title}>
            <h3 className="t-label">{t(g.title)}</h3>
            <dl>
              {g.rows.map(([keys, label]) => (
                <div key={keys.join('+')} className="shortcut-row">
                  <dt>{t(label)}</dt>
                  <dd dir="ltr">
                    {keys.map((k, i) => (
                      <span key={i}>
                        {i > 0 && <span className="plus">+</span>}
                        <kbd className="kbd">{k}</kbd>
                      </span>
                    ))}
                  </dd>
                </div>
              ))}
            </dl>
          </section>
        ))}
        <p className="t-meta">{t('shortcuts.dividerNote')}</p>
      </div>
    </Dialog>
  );
}
