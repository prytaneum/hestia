import type { Edition } from '@/api';
import { Badge } from '@/components/ui/badge';
import { editionLabel } from '../lib/edition';

/** Names the edition when it is not the default one, so Java screens stay unchanged. */
export function EditionBadge({ edition }: { edition?: Edition }) {
  if (!edition || edition === 'java') return null;
  return (
    <Badge variant="outline" className="shrink-0">
      {editionLabel[edition]()}
    </Badge>
  );
}
