import { useState, useRef } from 'react';
import { Download } from 'lucide-react';
import api from '@/lib/api';
import { saveBlob } from '@/lib/saveFile';
import { showDownloadToast } from '@/components/DownloadToast';

/**
 * One button: take the backup.
 *
 * This page used to explain what a backup contains and how much smaller it is
 * than the database, above a button that exported only the O.S. cases as CSV.
 * An officer who wants a backup wants the file, not the explanation, and the
 * button they were given was not the backup.
 *
 * So: the whole archive — every register, every setting, every user — in one
 * encrypted file, saved wherever they choose. Nothing else on the page.
 */
export default function ExportData() {
  const [busy, setBusy] = useState(false);
  const [percent, setPercent] = useState<number | null>(null);
  const [downloaded, setDownloaded] = useState('');
  const [done, setDone] = useState('');
  const [error, setError] = useState('');
  const abort = useRef<AbortController | null>(null);

  const download = async () => {
    setDone(''); setError(''); setPercent(null); setDownloaded('');
    setBusy(true);
    abort.current = new AbortController();
    try {
      const res = await api.get('/backup/archive/download', {
        responseType: 'blob',
        timeout: 0,                       // a large register takes as long as it takes
        signal: abort.current.signal,
        onDownloadProgress: (evt) => {
          const mb = evt.loaded / (1024 * 1024);
          setDownloaded(`${mb.toFixed(1)} MB`);
          // The server cannot always say how large the file will be; without a
          // total there is still movement to show.
          setPercent(evt.total ? Math.round((evt.loaded / evt.total) * 100) : null);
        },
      });

      const name = `cops_backup_${new Date().toISOString().slice(0, 10)}.cops`;
      const saved = await saveBlob(res.data as Blob, name, {
        title: 'Save the backup', extensions: ['cops'],
      });
      if (saved.cancelled) { setDone(''); return; }

      setDone(`Saved to ${saved.path}`);
      showDownloadToast(`Backup saved to ${saved.path}`);
      // The monthly reminder counts a backup as taken only when a file exists.
      localStorage.setItem('cops_archive_reminder_done_month',
        `${new Date().getFullYear()}-${new Date().getMonth() + 1}`);
      localStorage.removeItem('cops_archive_reminder_snoozed_until');
    } catch (err: unknown) {
      const e = err as { name?: string; code?: string; message?: string;
                         response?: { data?: unknown } };
      if (e?.name === 'CanceledError' || e?.code === 'ERR_CANCELED') return;
      let msg = e?.message || 'The backup could not be downloaded.';
      if (e?.response?.data instanceof Blob) {
        const text = await (e.response.data as Blob).text();
        try { msg = JSON.parse(text).detail || text; } catch { msg = text || msg; }
      }
      setError(msg);
    } finally {
      setBusy(false);
      setPercent(null);
      abort.current = null;
    }
  };

  return (
    <div className="max-w-lg mx-auto py-10 space-y-4">
      <div className="flex items-center gap-3">
        <button
          type="button"
          disabled={busy}
          onClick={download}
          className="flex items-center gap-2 px-5 py-2.5 text-sm font-semibold rounded-lg
                     bg-emerald-600 text-white hover:bg-emerald-700 disabled:opacity-60"
        >
          <Download size={16} />
          {busy ? 'Preparing…' : 'Download backup'}
        </button>
        {busy && (
          <button
            type="button"
            onClick={() => abort.current?.abort()}
            className="px-3 py-2 text-xs rounded-lg border border-slate-300 text-slate-600 hover:bg-slate-50"
          >
            Cancel
          </button>
        )}
      </div>

      {busy && (
        <div className="space-y-1">
          <div className="w-full bg-slate-200 rounded-full h-2 overflow-hidden">
            {percent === null ? (
              // No total to measure against — a moving bar, so the officer can
              // see it is working rather than wonder whether it has hung.
              <div className="bg-emerald-500 h-2 w-1/3 rounded-full animate-pulse" />
            ) : (
              <div
                className="bg-emerald-500 h-2 rounded-full transition-all duration-300"
                style={{ width: `${percent}%` }}
              />
            )}
          </div>
          <p className="text-xs text-slate-500">
            {percent !== null ? `${percent}% · ` : ''}{downloaded}
          </p>
        </div>
      )}

      {done  && <p className="text-xs text-emerald-700">{done}</p>}
      {error && <p className="text-xs text-red-600">{error}</p>}
    </div>
  );
}
