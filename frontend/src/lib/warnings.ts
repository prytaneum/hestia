import { type WarningInfo, warningHint, warningMessage } from '@/api';
import { notify } from '@/lib/notify';

export function toastWarnings(warnings: WarningInfo[] | undefined): void {
  for (const warning of warnings ?? []) {
    notify.warning(warningMessage(warning), {
      description: warningHint(warning),
    });
  }
}
