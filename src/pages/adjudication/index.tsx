import { lazy, Suspense } from 'react';
import { Routes, Route } from 'react-router-dom';
import AdjudicationLayout from './AdjudicationLayout';
import AdjudicationList from './AdjudicationList';
import AdjudicatedList from './AdjudicatedList';
import UserManagement from '../users/UserManagement';
import ChangePassword from '../auth/ChangePassword';

// Lazy-load the heavy form components so navigation clicks are instant.
// The browser parses these JS chunks only when the route is first visited.
const AdjudicationForm = lazy(() => import('./AdjudicationForm'));
const OffenceForm = lazy(() => import('../offence/OffenceForm'));
const OfflineAdjudicationList = lazy(() => import('./OfflineAdjudicationList'));
const OfflineAdjudicationComplete = lazy(() => import('./OfflineAdjudicationComplete'));

const RouteSpinner = () => (
  <div className="flex items-center justify-center h-64 text-slate-400 text-sm">Loading…</div>
);

export default function AdjudicationModule() {
  return (
    <Routes>
      <Route element={<AdjudicationLayout />}>
        {/* Default: show pending cases list */}
        <Route index element={<AdjudicationList />} />
        {/* Pending cases list */}
        <Route path="pending" element={<AdjudicationList />} />
        {/* Already adjudicated cases */}
        <Route path="adjudicated" element={<AdjudicatedList />} />
        {/* Adjudication form for a specific case */}
        <Route path="case/:os_no/:os_year" element={<Suspense fallback={<RouteSpinner />}><AdjudicationForm /></Suspense>} />
        {/* Route for adjudicator to securely edit SDO fields */}
        <Route path="edit-sdo/:osNo/:osYear" element={<Suspense fallback={<RouteSpinner />}><OffenceForm /></Suspense>} />
        {/* Offline adjudication */}
        <Route path="offline-pending" element={<Suspense fallback={<RouteSpinner />}><OfflineAdjudicationList /></Suspense>} />
        <Route path="offline-case/:os_no/:os_year" element={<Suspense fallback={<RouteSpinner />}><OfflineAdjudicationComplete /></Suspense>} />
        {/* User Management */}
        <Route path="users" element={<UserManagement moduleType="adjudication" />} />
        {/* Change Password */}
        <Route path="change-password" element={<ChangePassword />} />
      </Route>
    </Routes>
  );
}
