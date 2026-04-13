import { useQuery } from '@tanstack/react-query';
import { queryKeys, fetchers } from '@/lib/queries';

interface AppMode {
  isProd: boolean;
  isLoading: boolean;
}

export function useAppMode(): AppMode {
  const { data, isLoading } = useQuery({
    queryKey: queryKeys.mode(),
    queryFn: fetchers.mode,
    // Mode never changes during a session — cache it for the lifetime of the app.
    staleTime: Infinity,
    // On error assume prod (safe fallback — restrictive rather than permissive).
    placeholderData: { prod_mode: true },
  });

  return {
    isProd: data?.prod_mode ?? true,
    isLoading,
  };
}
