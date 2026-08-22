import { lazy, Suspense } from 'react';
import { Routes, Route, Navigate } from 'react-router-dom';
import SDOLayout from './SDOLayout';
import OffenceList from '../offence/OffenceList';
import ChangePassword from '../auth/ChangePassword';

const OffenceForm = lazy(() => import('../offence/OffenceForm'));
const OfflineAdjudicationForm = lazy(() => import('./OfflineAdjudicationForm'));
const RouteSpinner = () => (
  <div className="flex items-center justify-center h-64 text-slate-400 text-sm">Loading…</div>
);

export default function SDOModule() {
  return (
    <Routes>
      <Route element={<SDOLayout />}>
        <Route index element={<Suspense fallback={<RouteSpinner />}><OffenceForm /></Suspense>} />
        <Route path="offence" element={<OffenceList />} />
        <Route path="offence/new" element={<Suspense fallback={<RouteSpinner />}><OffenceForm /></Suspense>} />
        <Route path="offline-adjudication" element={<Suspense fallback={<RouteSpinner />}><OfflineAdjudicationForm /></Suspense>} />
        <Route path="offline-adjudication/:osNo/:osYear/edit" element={<Suspense fallback={<RouteSpinner />}><OfflineAdjudicationForm /></Suspense>} />
        <Route path="offence/:osNo/:osYear/edit" element={<Suspense fallback={<RouteSpinner />}><OffenceForm /></Suspense>} />
        <Route path="offence/:osNo/:osYear/view" element={<Suspense fallback={<RouteSpinner />}><OffenceForm /></Suspense>} />
        <Route path="change-password" element={<ChangePassword />} />
        {/* Any stray /sdo/* path — e.g. the removed Manage Users bookmark —
            lands back on the module home rather than a blank frame. */}
        <Route path="*" element={<Navigate to="/sdo" replace />} />
      </Route>
    </Routes>
  );
}
