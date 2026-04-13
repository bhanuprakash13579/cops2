import { useQuery, keepPreviousData } from '@tanstack/react-query';
import { queryKeys, fetchers, type OsListParams } from '@/lib/queries';

/**
 * Shared data hook for all OS case list pages (SDO list, Adjudication list,
 * Adjudicated list, Offline list, Quashed list).
 *
 * Uses `keepPreviousData` so the current page stays visible while the next
 * page loads — no blank flash on pagination or filter change.
 */
export function useOsList(params: OsListParams) {
  return useQuery({
    queryKey: queryKeys.osList(params),
    queryFn: () => fetchers.osList(params),
    placeholderData: keepPreviousData,
  });
}
