import type { MessageKey } from '../i18n/en';
import { isIpcError } from './commands';

export interface UiError {
  key: MessageKey;
  reference?: string;
}

/** Maps any thrown value to a localized message key plus the log correlation id. */
export function toUiError(e: unknown): UiError {
  if (isIpcError(e)) {
    return { key: `error.${e.code}` as MessageKey, reference: e.correlationId };
  }
  console.error(e);
  return { key: 'error.internal' };
}
