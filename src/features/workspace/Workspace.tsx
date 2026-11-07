import { useAppStore } from '../../app/store';
import { AskHeader, Composer, ScopeBar } from '../chat/AskChrome';
import { ConversationPane } from '../chat/Conversation';
import { ComparePane } from '../comparison/ComparePane';
import { DocumentDetail } from './DocumentDetail';
import { EmptyLibrary } from './EmptyLibrary';
import { Overview } from './Overview';
import './workspace.css';

export function Workspace() {
  const tab = useAppStore((s) => s.tab);
  const total = useAppStore((s) => s.documentsTotal);
  const filter = useAppStore((s) => s.filter);
  const center = useAppStore((s) => s.center);
  const selected = useAppStore((s) => s.documents.find((d) => d.id === s.selectedDocumentId) ?? null);

  if (tab === 'compare') {
    return (
      <main className="workspace" data-region="answer" tabIndex={-1}>
        <ComparePane />
      </main>
    );
  }
  const empty = total === 0 && !filter;
  if (empty) {
    return (
      <main className="workspace" data-region="answer" tabIndex={-1}>
        <div className="workspace-body scroll">
          <EmptyLibrary />
        </div>
        <Composer />
      </main>
    );
  }
  return (
    <main className="workspace" data-region="answer" tabIndex={-1}>
      <AskHeader />
      <ScopeBar />
      <div className="workspace-body scroll">
        {center === 'document' && selected ? <DocumentDetail doc={selected} /> : center === 'conversation' ? <ConversationPane /> : <Overview />}
      </div>
      <Composer />
    </main>
  );
}
