import { QueryClient } from '@tanstack/react-query';

/**
 * Shared QueryClient instance for the entire app.
 *
 * Tuned for a localhost Tauri desktop app:
 * - staleTime 30s  — data considered fresh for 30 seconds; no background refetch within that window.
 * - gcTime 5min    — unused cache entries held for 5 minutes (same as default).
 * - retry 1        — one retry on failure (backend is local so transient errors are rare).
 * - refetchOnWindowFocus false — app is a desktop tool, window focus events are noisy.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30 * 1000,
      gcTime: 5 * 60 * 1000,
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
});
