/** A shared request lane: an invalidated request must settle before its replacement starts. */
export function createPollingController<Config, Value>(options: {
  fetch: (config: Config) => Promise<Value>;
  onValue: (value: Value) => void;
  onError: (error: unknown) => void;
  onLoading?: (loading: boolean) => void;
  schedule?: (callback: () => void, ms: number) => () => void;
}) {
  let config: Config;
  let active = false;
  let generation = 0;
  let requested = false;
  let runningGeneration = -1;
  let inFlight: Promise<void> | null = null;
  let cancelTimer: (() => void) | undefined;
  const schedule = options.schedule ?? ((callback, ms) => {
    const timer = setInterval(callback, ms);
    return () => clearInterval(timer);
  });

  const refresh = (): Promise<void> => {
    if (!active) return Promise.resolve();
    // Timer ticks and manual refreshes share the currently running request.
    if (inFlight) {
      // A manual request after reconfiguration waits behind the invalidated lane.
      if (runningGeneration !== generation) requested = true;
      return inFlight;
    }
    requested = true;
    runningGeneration = generation;
    inFlight = Promise.resolve().then(async () => {
      try {
        while (active && requested) {
          requested = false;
          const requestGeneration = generation;
          runningGeneration = requestGeneration;
          const requestConfig = config;
          options.onLoading?.(true);
          try {
            const value = await options.fetch(requestConfig);
            if (active && generation === requestGeneration) options.onValue(value);
          } catch (error) {
            if (active && generation === requestGeneration) options.onError(error);
          } finally {
            if (active && generation === requestGeneration) options.onLoading?.(false);
          }
        }
      } finally {
        // Clear in the same microtask that exits the loop. A separate .finally
        // would leave a gap where a config change queues work on a finished loop.
        inFlight = null;
      }
    });
    return inFlight;
  };

  return {
    configure(nextConfig: Config, intervalMs: number, mode: { immediate?: boolean; automatic?: boolean } = {}) {
      cancelTimer?.();
      config = nextConfig;
      active = true;
      generation += 1;
      // A setting/period change schedules exactly one replacement after the old
      // physical request settles; it must never reset the in-flight lock.
      requested = mode.immediate ?? true;
      options.onLoading?.(requested);
      if (requested) void refresh();
      cancelTimer = mode.automatic === false ? undefined : schedule(() => { void refresh(); }, intervalMs);
    },
    refresh,
    stop() {
      active = false;
      generation += 1;
      requested = false;
      cancelTimer?.();
      cancelTimer = undefined;
    },
  };
}

/** Unlike Promise.all, a rejection cannot release a lane while its sibling runs. */
export async function settlePair<A, B>(first: Promise<A>, second: Promise<B>) {
  const [a, b] = await Promise.allSettled([first, second]);
  return { first: a, second: b };
}
