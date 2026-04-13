import { useEffect, useState, useCallback, useRef } from 'react';
import { CheckCircle, X, FolderOpen, AlertCircle, Loader2 } from 'lucide-react';

// ── Shared download-progress state (module-level singleton) ──────────────────
// Any component can push progress events; the single <DownloadToast /> rendered
// in the root layout renders a progress bar + "Open in PDF Viewer" button.
// Zero layout shift, zero interaction blocking, supports parallel downloads.

type DownloadStatus = 'downloading' | 'done' | 'failed';

type DownloadEntry = {
  id: number;
  key: string;
  label: string;
  status: DownloadStatus;
  progress: number; // 0-100
  filePath?: string; // only set on 'done' for the Open button
};

let _nextId = 0;
let _entries: DownloadEntry[] = [];
let _listeners: Array<(entries: DownloadEntry[]) => void> = [];

function _notify() {
  _listeners.forEach(fn => fn([..._entries]));
}

/** Register a new download — shows progress bar immediately. */
export function startDownload(key: string, label: string) {
  if (_entries.some(e => e.key === key)) return; // already tracked
  _entries.push({ id: ++_nextId, key, label, status: 'downloading', progress: 0 });
  if (_entries.length > 5) _entries = _entries.slice(-5);
  _notify();
}

/** Update download progress (0–100). Never decreases existing value. */
export function progressDownload(key: string, percent: number) {
  _entries = _entries.map(e =>
    e.key === key ? { ...e, progress: Math.min(100, Math.max(e.progress, Math.round(percent))) } : e
  );
  _notify();
}

/**
 * Mark download complete.
 * filePath — native path on disk; if provided, "Open in PDF Viewer" button appears.
 */
export function completeDownload(key: string, filePath?: string) {
  _entries = _entries.map(e =>
    e.key === key ? { ...e, status: 'done', progress: 100, filePath } : e
  );
  _notify();
  // Auto-dismiss after 6s — enough time for user to notice + click Open
  setTimeout(() => {
    _entries = _entries.filter(e => e.key !== key);
    _notify();
  }, 6000);
}

/** Mark download failed — auto-dismisses in 3s. */
export function failDownload(key: string) {
  _entries = _entries.map(e =>
    e.key === key ? { ...e, status: 'failed' } : e
  );
  _notify();
  setTimeout(() => {
    _entries = _entries.filter(e => e.key !== key);
    _notify();
  }, 3000);
}

/** Backward-compat: simple success toast (no progress bar, no Open button). */
export function showDownloadToast(message: string) {
  const key = `simple-${++_nextId}`;
  _entries.push({ id: _nextId, key, label: message, status: 'done', progress: 100 });
  if (_entries.length > 5) _entries = _entries.slice(-5);
  _notify();
  setTimeout(() => {
    _entries = _entries.filter(e => e.key !== key);
    _notify();
  }, 4000);
}

/** Render this ONCE near the app root (e.g. in AppLayout). */
export default function DownloadToast() {
  const [entries, setEntries] = useState<DownloadEntry[]>([]);
  const ref = useRef(setEntries);
  ref.current = setEntries;

  useEffect(() => {
    const listener = (e: DownloadEntry[]) => ref.current(e);
    _listeners.push(listener);
    return () => { _listeners = _listeners.filter(l => l !== listener); };
  }, []);

  const dismiss = useCallback((key: string) => {
    _entries = _entries.filter(e => e.key !== key);
    _notify();
  }, []);

  const handleOpen = useCallback(async (filePath: string) => {
    try {
      const { open } = await import('@tauri-apps/plugin-shell');
      await open(filePath);
    } catch {
      // Non-Tauri env or open failed — silently ignore
    }
  }, []);

  if (entries.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-[9999] flex flex-col gap-2 pointer-events-none print:hidden">
      {entries.map(e => (
        <div
          key={e.key}
          className={`pointer-events-auto flex flex-col gap-2 text-sm font-medium pl-3 pr-2 py-2.5 rounded-lg shadow-lg animate-[slideInRight_0.25s_ease-out] w-72 ${
            e.status === 'failed'      ? 'bg-red-600 text-white'     :
            e.status === 'done'        ? 'bg-emerald-600 text-white' :
                                         'bg-slate-800 text-white'
          }`}
        >
          {/* Top row: icon + label + dismiss */}
          <div className="flex items-center gap-2">
            {e.status === 'downloading' && <Loader2 className="w-4 h-4 shrink-0 animate-spin" />}
            {e.status === 'done'        && <CheckCircle className="w-4 h-4 shrink-0" />}
            {e.status === 'failed'      && <AlertCircle className="w-4 h-4 shrink-0" />}
            <span className="flex-1 truncate text-xs">{e.label}</span>
            <button
              onClick={() => dismiss(e.key)}
              className="ml-1 p-0.5 rounded hover:bg-white/20 transition-colors shrink-0"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          </div>

          {/* Progress bar — only while downloading */}
          {e.status === 'downloading' && (
            <div className="h-1 bg-white/20 rounded-full overflow-hidden">
              <div
                className="h-full bg-brand-400 rounded-full transition-[width] duration-300 ease-out"
                style={{ width: `${e.progress}%` }}
              />
            </div>
          )}

          {/* Open button — only when done and we have a native path */}
          {e.status === 'done' && e.filePath && (
            <button
              onClick={() => handleOpen(e.filePath!)}
              className="flex items-center gap-1.5 text-xs bg-white/20 hover:bg-white/30 transition-colors rounded px-2 py-1 font-semibold self-start"
            >
              <FolderOpen className="w-3.5 h-3.5" /> Open in PDF Viewer
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
