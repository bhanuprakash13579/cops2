import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom';
import { useState, useEffect, useCallback, lazy, Suspense } from 'react';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { useQuery } from '@tanstack/react-query';
import { queryKeys, fetchers } from './lib/queries';
import api from './lib/api';
import DevModeBanner from './components/DevModeBanner';
import DownloadToast from './components/DownloadToast';
import ErrorBoundary from './components/ErrorBoundary';
import { AuthProvider, useAuth } from './contexts/AuthContext';
import Login from './pages/auth/Login';
import ModuleSelection from './pages/ModuleSelection';
// Lazy-loaded — defers JS parsing until the user actually navigates to that module.
// Each module is a large sub-tree; parsing all four on startup wastes ~300-500ms.
const SDOModule = lazy(() => import('./pages/sdo'));
const AdjudicationModule = lazy(() => import('./pages/adjudication'));
const QueryModule = lazy(() => import('./pages/query'));
const ApisModule = lazy(() => import('./pages/apis'));
const RestoreBackup = lazy(() => import('./pages/backup/RestoreBackup'));

// Minimal fallback shown while a lazy module chunk is loading (first navigation only)
function ModuleLoader() {
  return (
    <div className="min-h-screen flex items-center justify-center bg-slate-900">
      <div className="text-blue-300 text-sm animate-pulse">Loading module…</div>
    </div>
  );
}

// Checks for updates silently in the background 5 seconds after the app is
// ready. Shows a non-intrusive corner banner only when a newer version exists.
// All errors are swallowed — a failed update check must never affect the app.
function AutoUpdater() {
  const [update, setUpdate]         = useState<Update | null>(null);
  const [dismissed, setDismissed]   = useState(false);
  const [installing, setInstalling] = useState(false);

  useEffect(() => {
    // Only runs inside a real Tauri window — skip in browser dev mode.
    if (!('__TAURI_INTERNALS__' in window)) return;

    const timer = setTimeout(async () => {
      try {
        const result = await check();
        if (result?.available) setUpdate(result);
      } catch {
        // Network down, GitHub unreachable, dev build — all silently ignored.
      }
    }, 5_000);

    return () => clearTimeout(timer);
  }, []);

  const handleInstall = useCallback(async () => {
    if (!update) return;
    setInstalling(true);
    try {
      await update.downloadAndInstall();
      await relaunch();
    } catch {
      setInstalling(false);
    }
  }, [update]);

  if (!update || dismissed) return null;

  return (
    <div className="fixed bottom-5 right-5 z-[9999] w-80 bg-slate-800 border border-blue-500/60 rounded-xl shadow-2xl p-4 animate-in slide-in-from-bottom-4 fade-in duration-300">
      <div className="flex items-start gap-3">
        <div className="mt-0.5 flex-shrink-0 w-8 h-8 rounded-full bg-blue-600/20 flex items-center justify-center">
          <svg className="w-4 h-4 text-blue-400" fill="none" viewBox="0 0 24 24" stroke="currentColor" strokeWidth={2}>
            <path strokeLinecap="round" strokeLinejoin="round" d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
          </svg>
        </div>
        <div className="flex-1 min-w-0">
          <p className="text-white text-sm font-semibold">Update available</p>
          <p className="text-slate-400 text-xs mt-0.5">COPS {update.version} is ready to install.</p>
          <div className="flex gap-2 mt-3">
            <button
              onClick={handleInstall}
              disabled={installing}
              className="flex-1 bg-blue-600 hover:bg-blue-500 disabled:bg-blue-800 disabled:cursor-not-allowed text-white text-xs font-medium py-1.5 rounded-lg transition-colors"
            >
              {installing ? 'Installing…' : 'Update & Restart'}
            </button>
            <button
              onClick={() => setDismissed(true)}
              disabled={installing}
              className="px-3 text-slate-400 hover:text-slate-200 text-xs transition-colors"
            >
              Later
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// SDO-only guard
function SDORoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, canAccessSDO } = useAuth();
  if (!isAuthenticated) return <Navigate to="/login/sdo" replace />;
  if (!canAccessSDO()) return <Navigate to="/login/sdo" replace />;
  return <>{children}</>;
}

// Adjudication-only guard
function AdjudicationRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, canAccessAdjudication } = useAuth();
  if (!isAuthenticated) return <Navigate to="/login/adjudication" replace />;
  if (!canAccessAdjudication()) return <Navigate to="/login/adjudication" replace />;
  return <>{children}</>;
}

// Query guard (allows SDO and Adjn roles)
function QueryRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, canAccessQuery } = useAuth();
  if (!isAuthenticated) return <Navigate to="/login/query" replace />;
  if (!canAccessQuery()) return <Navigate to="/login/query" replace />;
  return <>{children}</>;
}

// APIS guard (SDO and Adjn roles + feature flag must be enabled)
function ApisRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated, canAccessApis } = useAuth();
  const { data, isLoading } = useQuery({
    queryKey: queryKeys.features(),
    queryFn: fetchers.features,
    staleTime: Infinity, // feature flags don't change during a session
  });

  if (isLoading) return <div className="min-h-screen bg-slate-900" />;
  if (!data?.apis_enabled) return <Navigate to="/modules" replace />;
  if (!isAuthenticated) return <Navigate to="/login/apis" replace />;
  if (!canAccessApis()) return <Navigate to="/login/apis" replace />;
  return <>{children}</>;
}

function AppRoutes() {
  return (
    <Suspense fallback={<ModuleLoader />}>
    <Routes>
      {/* Public Landing Page */}
      <Route path="/modules" element={<ModuleSelection />} />

      {/* Dynamic Login Page for specific modules */}
      <Route path="/login/:moduleType" element={<Login />} />

      {/* SDO Module — SDO role only */}
      <Route
        path="/sdo/*"
        element={
          <SDORoute>
            <SDOModule />
          </SDORoute>
        }
      />

      {/* Adjudication Module — DC and AC only */}
      <Route
        path="/adjudication/*"
        element={
          <AdjudicationRoute>
            <AdjudicationModule />
          </AdjudicationRoute>
        }
      />

      {/* Query Module — Cross Role Access */}
      <Route
        path="/query/*"
        element={
          <QueryRoute>
            <QueryModule />
          </QueryRoute>
        }
      />

      {/* COPS ↔ APIS Module — SDO and Adjn roles */}
      <Route
        path="/apis/*"
        element={
          <ApisRoute>
            <ApisModule />
          </ApisRoute>
        }
      />

      {/* Hidden Admin Panel — no pre-auth needed; the page itself requires sysadmin credentials */}
      <Route path="/restore-backup" element={<RestoreBackup />} />

      {/* Root → always redirect to public module selection */}
      <Route path="/" element={<Navigate to="/modules" replace />} />
      <Route path="/login" element={<Navigate to="/modules" replace />} />
      <Route path="*" element={<Navigate to="/modules" replace />} />
    </Routes>
    </Suspense>
  );
}

// Polls /api/mode until the Python backend is ready, then renders the app.
// Without this, the window opens immediately while the sidecar is still
// running startup migrations and seed queries — resulting in blank pages
// and API errors for the first few seconds.
const SPLASH_BG = { background: 'linear-gradient(135deg, #0f172a 0%, #1e3a5f 50%, #0f172a 100%)' };

function BackendGate({ children }: { children: React.ReactNode }) {
  const [ready, setReady] = useState(false);
  const [fadeOut, setFadeOut] = useState(false);
  const [dots, setDots] = useState('');
  const [slowStart, setSlowStart] = useState(false);
  const [startupError, setStartupError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let dotTimer: ReturnType<typeof setInterval>;
    const slowTimer = setTimeout(() => { if (!cancelled) setSlowStart(true); }, 15000);

    // Listen for the Tauri "sidecar-startup-failed" event emitted by lib.rs
    // when the backend crashes 4+ times in quick succession.
    let unlistenFn: (() => void) | undefined;
    if (typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window) {
      import('@tauri-apps/api/event').then(({ listen }) => {
        listen<string>('sidecar-startup-failed', (event) => {
          if (!cancelled) setStartupError(event.payload);
        }).then(fn => { unlistenFn = fn; });
      });
    }

    const poll = async () => {
      // Fast path: if backend is already up (e.g. app restart), skip splash entirely
      try {
        await api.get('/mode', { timeout: 500 });
        if (!cancelled) setReady(true);
        return;
      } catch { /* not ready yet — fall through to normal polling with splash */ }

      // Backend is starting — show splash and poll until it responds
      dotTimer = setInterval(() => setDots((d: string) => d.length >= 3 ? '' : d + '.'), 500);
      while (!cancelled) {
        try {
          await api.get('/mode', { timeout: 5000 });
          if (!cancelled) {
            setFadeOut(true);
            setTimeout(() => { if (!cancelled) setReady(true); }, 300);
          }
          return;
        } catch {
          await new Promise(r => setTimeout(r, 800));
        }
      }
    };

    poll();
    return () => {
      cancelled = true;
      clearInterval(dotTimer);
      clearTimeout(slowTimer);
      unlistenFn?.();
    };
  }, []);

  if (!ready) {
    if (startupError) {
      return (
        <div className="min-h-screen flex flex-col items-center justify-center" style={SPLASH_BG}>
          <img src="/cops_logo.png" alt="COPS" className="w-20 h-20 object-contain mb-4 opacity-70" />
          <p className="text-white text-lg font-semibold tracking-widest mb-4">COPS</p>
          <div className="bg-red-900/60 border border-red-500 rounded-lg px-6 py-4 max-w-lg mx-4 text-center">
            <p className="text-red-300 font-semibold mb-2">Backend failed to start</p>
            <p className="text-red-200 text-sm leading-relaxed">{startupError}</p>
          </div>
          <button
            className="mt-6 px-4 py-2 bg-blue-700 hover:bg-blue-600 text-white text-sm rounded"
            onClick={() => window.location.reload()}
          >
            Retry
          </button>
        </div>
      );
    }
    return (
      <div
        className="min-h-screen flex flex-col items-center justify-center"
        style={{ ...SPLASH_BG, opacity: fadeOut ? 0 : 1, transition: 'opacity 0.3s ease' }}
      >
        <img src="/cops_logo.png" alt="COPS" className="w-24 h-24 object-contain mb-6 opacity-90" />
        <p className="text-white text-lg font-semibold tracking-widest">COPS</p>
        {slowStart
          ? <p className="text-yellow-300 text-sm mt-2 text-center px-8">First launch: setting up database, please wait{dots}</p>
          : <p className="text-blue-300 text-sm mt-2 w-32 text-center">Starting{dots}</p>
        }
      </div>
    );
  }

  return <>{children}</>;
}

export default function App() {
  return (
    <ErrorBoundary>
      <AuthProvider>
        <BackendGate>
          <AutoUpdater />
          <DevModeBanner />
          <DownloadToast />
          <BrowserRouter>
            <AppRoutes />
          </BrowserRouter>
        </BackendGate>
      </AuthProvider>
    </ErrorBoundary>
  );
}
