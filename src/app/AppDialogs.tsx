import { useState } from 'react';
import { useAppStore, type DialogState } from './store';
import { api } from '../shared/ipc/commands';
import { useT } from '../shared/i18n';
import { Dialog } from '../shared/ui/Dialog';

export function AppDialogs() {
  const dialog = useAppStore((s) => s.dialog);
  if (!dialog) return null;
  switch (dialog.kind) {
    case 'createCollection':
      return <CollectionNameDialog mode="create" then={dialog.thenImport} />;
    case 'renameCollection':
      return <CollectionNameDialog mode="rename" id={dialog.id} initial={dialog.name} />;
    case 'deleteCollection':
      return <DeleteCollectionDialog d={dialog} />;
    case 'deleteDocument':
      return <DeleteDocumentDialog d={dialog} />;
    case 'pickCollectionForImport':
      return <PickCollectionDialog d={dialog} />;
  }
}

function CollectionNameDialog({
  mode,
  id,
  initial = '',
  then,
}: {
  mode: 'create' | 'rename';
  id?: string;
  initial?: string;
  then?: Extract<DialogState, { kind: 'createCollection' }>['thenImport'];
}) {
  const t = useT();
  const setDialog = useAppStore((s) => s.setDialog);
  const create = useAppStore((s) => s.createCollection);
  const rename = useAppStore((s) => s.renameCollection);
  const importSelection = useAppStore((s) => s.importSelection);
  const [name, setName] = useState(initial);
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    if (mode === 'create') {
      const c = await create(name);
      setBusy(false);
      if (!c) return;
      setDialog(null);
      if (then) void importSelection(then, c.id);
    } else if (id) {
      const ok = await rename(id, name);
      setBusy(false);
      if (ok) setDialog(null);
    }
  }

  return (
    <Dialog
      title={t(mode === 'create' ? 'collection.createTitle' : 'collection.renameTitle')}
      onClose={() => setDialog(null)}
    >
      <form onSubmit={(e) => void submit(e)} className="dialog-form">
        {then && <p>{t('collection.needOneFirst')}</p>}
        <label>
          <span className="sr-only">{t('collection.namePlaceholder')}</span>
          <input className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder={t('collection.namePlaceholder')} maxLength={80} dir="auto" data-autofocus />
        </label>
        <p className="t-meta">{t('collection.nameHelp')}</p>
        <div className="dialog-actions">
          <button type="button" className="btn" onClick={() => setDialog(null)}>
            {t('common.cancel')}
          </button>
          <button type="submit" className="btn btn-primary" disabled={busy || !name.trim()}>
            {t(mode === 'create' ? 'collection.create' : 'collection.save')}
          </button>
        </div>
      </form>
    </Dialog>
  );
}

function DeleteCollectionDialog({ d }: { d: Extract<DialogState, { kind: 'deleteCollection' }> }) {
  const t = useT();
  const setDialog = useAppStore((s) => s.setDialog);
  const del = useAppStore((s) => s.deleteCollection);
  return (
    <Dialog
      title={t('collection.deleteTitle', { name: d.name })}
      onClose={() => setDialog(null)}
      footer={
        <>
          <button className="btn" onClick={() => setDialog(null)} data-autofocus>
            {t('common.cancel')}
          </button>
          <button
            className="btn btn-danger"
            onClick={() => {
              setDialog(null);
              void del(d.id);
            }}
          >
            {t('collection.delete')}
          </button>
        </>
      }
    >
      <p>{t('collection.deleteBody')}</p>
    </Dialog>
  );
}

function DeleteDocumentDialog({ d }: { d: Extract<DialogState, { kind: 'deleteDocument' }> }) {
  const t = useT();
  const setDialog = useAppStore((s) => s.setDialog);
  const fail = useAppStore((s) => s.fail);
  const refresh = useAppStore((s) => s.refreshDocuments);
  const refreshCollections = useAppStore((s) => s.refreshCollections);
  const reader = useAppStore((s) => s.reader);
  const closeReader = useAppStore((s) => s.closeReader);
  const select = useAppStore((s) => s.selectDocument);
  async function confirm() {
    setDialog(null);
    try {
      if (reader?.documentId === d.id) closeReader();
      select(null);
      await api.deleteDocument(d.id);
      await Promise.all([refresh(), refreshCollections()]);
    } catch (e) {
      fail(e);
    }
  }
  return (
    <Dialog
      title={t('doc.deleteTitle')}
      onClose={() => setDialog(null)}
      footer={
        <>
          <button className="btn" onClick={() => setDialog(null)} data-autofocus>
            {t('common.cancel')}
          </button>
          <button className="btn btn-danger" onClick={() => void confirm()}>
            {t('doc.deleteConfirm')}
          </button>
        </>
      }
    >
      <p className="t-paper-title" dir="auto">
        {d.title}
      </p>
      <p>{t('doc.deleteBody')}</p>
    </Dialog>
  );
}

function PickCollectionDialog({ d }: { d: Extract<DialogState, { kind: 'pickCollectionForImport' }> }) {
  const t = useT();
  const collections = useAppStore((s) => s.collections);
  const setDialog = useAppStore((s) => s.setDialog);
  const importSelection = useAppStore((s) => s.importSelection);
  return (
    <Dialog title={t('library.chooseCollection')} onClose={() => setDialog(null)}>
      <div className="pick-list">
        {collections.map((c) => (
          <button
            key={c.id}
            className="menu-item"
            onClick={() => {
              setDialog(null);
              void importSelection(d.selection, c.id);
            }}
          >
            <span dir="auto">{c.name}</span>
            <span className="count">{c.documentCount}</span>
          </button>
        ))}
        <div className="menu-sep" />
        <button className="menu-item" onClick={() => setDialog({ kind: 'createCollection', thenImport: d.selection })}>
          {t('library.newCollection')}
        </button>
      </div>
    </Dialog>
  );
}
