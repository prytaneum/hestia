/**
 * Live resource metrics for one supervised process: a rolling series
 * accumulated from the daemon's `process.metrics` broadcast, filtered to the
 * given id. The latest sample and the windowed history drive the overview
 * charts. Empty when the process is not running (no samples arrive).
 */
import { useRef, useState } from 'react';
import type { ProcessMetrics } from '../api';
import { useDaemonEvent } from './events';

export interface MetricSample {
  cpuPct: number;
  memBytes: number;
}

export interface ProcessMetricsResult {
  series: MetricSample[];
  latest?: MetricSample;
}

export function useProcessMetrics(
  processId: string | null,
  window = 60,
): ProcessMetricsResult {
  const [series, setSeries] = useState<MetricSample[]>([]);
  const idRef = useRef(processId);

  if (idRef.current !== processId) {
    idRef.current = processId;
    setSeries([]);
  }

  useDaemonEvent('process.metrics', (payload) => {
    if (!processId) return;
    const sample = payload.samples.find(
      (m: ProcessMetrics) => m.id === processId,
    );
    if (!sample) return;
    const next: MetricSample = {
      cpuPct: sample.cpuPct,
      memBytes: sample.memBytes,
    };
    setSeries((prev) =>
      prev.length >= window ? [...prev.slice(1), next] : [...prev, next],
    );
  });

  return { series, latest: series[series.length - 1] };
}
