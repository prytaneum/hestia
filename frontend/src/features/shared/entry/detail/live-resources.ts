import type { ConfigEntry } from '@/api';
import { memGb } from '@/lib/format';
import { useProcessMetrics } from '@/queries/metrics';
import type { LiveResources } from '../components/resource-panel';

const DEFAULT_MEMORY_GB = 4;

export function useLiveResources({
  running,
  config,
  diskBytes,
  processId,
}: {
  running: boolean;
  config?: ConfigEntry[];
  diskBytes?: number;
  processId: string | null;
}): { live: LiveResources; memoryLimitGb: number } {
  const metrics = useProcessMetrics(processId);
  const memory = config?.find((e) => e.key === 'memory')?.value;
  const memoryLimitGb = memory ? memGb(memory) : DEFAULT_MEMORY_GB;

  return {
    memoryLimitGb,
    live: {
      running,
      memoryLimitGb,
      diskBytes: diskBytes ?? 0,
      series: metrics.series.map((s) => ({
        cpu: s.cpuPct,
        mem: s.memBytes / (1024 * 1024),
      })),
    },
  };
}
