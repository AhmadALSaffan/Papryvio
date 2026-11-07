// Typed event subscriptions. Every event arrives in an envelope with a monotonic sequence.
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type { DocumentChangedPayload } from '../types/generated/DocumentChangedPayload';
import type { DragStatePayload } from '../types/generated/DragStatePayload';
import type { EventEnvelope } from '../types/generated/EventEnvelope';
import type { ImportSelection } from '../types/generated/ImportSelection';
import type { JobView } from '../types/generated/JobView';
import type { GenerationPayload } from '../types/generated/GenerationPayload';
import type { ComparisonPayload } from '../types/generated/ComparisonPayload';

export const EVENT_SCHEMA_VERSION = 1;

type EventMap = {
  job_progress: JobView;
  document_changed: DocumentChangedPayload;
  library_changed: Record<string, never>;
  files_dropped: ImportSelection;
  drag_state: DragStatePayload;
  generation_progress: GenerationPayload;
  generation_finished: GenerationPayload;
  comparison_updated: ComparisonPayload;
  models_changed: Record<string, never>;
};

export function on<K extends keyof EventMap>(
  name: K,
  handler: (payload: EventMap[K], envelope: EventEnvelope<EventMap[K]>) => void,
): Promise<UnlistenFn> {
  return listen<EventEnvelope<EventMap[K]>>(name, (e) => {
    if (e.payload.schemaVersion !== EVENT_SCHEMA_VERSION) {
      console.warn(`ignoring ${name} with schema v${e.payload.schemaVersion}`);
      return;
    }
    handler(e.payload.payload, e.payload);
  });
}
