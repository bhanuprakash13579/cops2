import { useState, useEffect } from 'react';
import api from '@/lib/api';

export interface TrialStatus {
  trial_disabled: boolean;
  days_remaining: number | null;
  trial_days: number;
  expired: boolean;
  isLoading: boolean;
}

// Module-level cache — trial status doesn't change mid-session
let _cache: Omit<TrialStatus, 'isLoading'> | null = null;

export function useTrialStatus(): TrialStatus {
  const [status, setStatus] = useState<Omit<TrialStatus, 'isLoading'>>(
    _cache ?? { trial_disabled: false, days_remaining: 30, trial_days: 30, expired: false }
  );
  const [isLoading, setIsLoading] = useState(_cache === null);

  useEffect(() => {
    if (_cache !== null) return;
    api.get('/trial-status')
      .then(r => {
        _cache = {
          trial_disabled: !!r.data.trial_disabled,
          days_remaining: r.data.days_remaining ?? null,
          trial_days:     r.data.trial_days ?? 30,
          expired:        !!r.data.expired,
        };
        setStatus(_cache);
      })
      .catch(() => {
        // On error assume not expired — don't block the app
        _cache = { trial_disabled: true, days_remaining: null, trial_days: 30, expired: false };
        setStatus(_cache);
      })
      .finally(() => setIsLoading(false));
  }, []);

  return { ...status, isLoading };
}
