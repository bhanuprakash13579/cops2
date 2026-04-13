/**
 * Central query keys and fetcher functions for TanStack Query.
 *
 * Convention:
 *   queryKeys.thing()          → key for a collection / config
 *   queryKeys.thing(id, ...)   → key for a specific item
 *
 * Fetchers are plain async functions that accept the same params used in the key.
 * Keeping keys and fetchers here means you only update one place when an endpoint changes.
 */
import api from './api';

// ── Query Keys ────────────────────────────────────────────────────────────────

export const queryKeys = {
  // App-level
  mode:       () => ['mode']               as const,
  features:   () => ['features']          as const,
  // OS cases list — key includes all filter params so each unique filter set is cached separately
  osList: (params: OsListParams) => ['os', 'list', params] as const,
  osDetail:   (osNo: string, osYear: number) => ['os', osNo, osYear]   as const,
  offlinePending: () => ['os', 'offline-pending'] as const,
  // Dashboard
  dashboard:  () => ['dashboard', 'stats'] as const,
  sidebarCounts: () => ['os', 'sidebar-counts'] as const,
  // Registers
  brList: (params: Record<string, any>) => ['br', 'list', params] as const,
  drList: (params: Record<string, any>) => ['dr', 'list', params] as const,
};

// ── Param types ───────────────────────────────────────────────────────────────

export interface OsListParams {
  page: number;
  per_page: number;
  search: string;
  status?: string;
  year?: string | number;
  br_dr_pending?: boolean;
}

// ── Fetchers ──────────────────────────────────────────────────────────────────

export const fetchers = {
  mode: async () => {
    const r = await api.get('/mode');
    return r.data as { prod_mode: boolean };
  },

  features: async () => {
    const r = await api.get('/features');
    return r.data as { apis_enabled: boolean; [key: string]: unknown };
  },

  osList: async (params: OsListParams) => {
    const r = await api.get('/os/', { params });
    return r.data as { items: any[]; total: number };
  },

  osDetail: async (osNo: string, osYear: number) => {
    const r = await api.get(`/os/${osNo}/${osYear}`);
    return r.data;
  },

  offlinePending: async () => {
    const r = await api.get('/os/offline-pending');
    return (Array.isArray(r.data) ? r.data : (r.data?.items ?? [])) as any[];
  },

  dashboard: async () => {
    const r = await api.get('/dashboard/stats');
    return r.data;
  },

  sidebarCounts: async () => {
    const r = await api.get('/os/sidebar-counts');
    return r.data as { pending: number; offline_pending: number };
  },

  brList: async (params: Record<string, any>) => {
    const r = await api.get('/br', { params });
    return r.data as { items: any[]; total: number };
  },

  drList: async (params: Record<string, any>) => {
    const r = await api.get('/dr', { params });
    return r.data as { items: any[]; total: number };
  },
};
